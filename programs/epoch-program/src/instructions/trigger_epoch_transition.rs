//! trigger_epoch_transition instruction.
//!
//! Initiates an epoch transition by validating the epoch boundary is reached,
//! issuing an ORAO VRF randomness request by CPI, binding the resulting request
//! PDA, and paying the trigger bounty.
//!
//! ORAO's flow is TWO of our transactions, not three:
//!   TX 1 (this instruction): request randomness via `orao_vrf::request_v2` CPI
//!   ...   ORAO's fulfilment nodes land their own transaction(s); we send nothing
//!   TX 2: `consume_randomness`
//! There is no client-side account creation and no SDK `commitIx`: ORAO `init`s
//! the request PDA inside our CPI. The caller only has to LIST that address,
//! which it can derive off-chain because `epoch_vrf_seed` is clock-free.
//! Source: Epoch_State_Machine_Spec.md Section 8.2; 179.1-RESEARCH.md Pattern 1

use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_lang::solana_program::rent::Rent;
use anchor_lang::solana_program::system_instruction;
use anchor_lang::solana_program::sysvar::Sysvar;
use orao_solana_vrf::program::OraoVrf;
use orao_solana_vrf::state::NetworkState;
use orao_solana_vrf::CONFIG_ACCOUNT_SEED;

use crate::constants::{
    ARB_CONFIG_SEED, CARNAGE_SOL_VAULT_SEED, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED, GRACE_SLOTS,
    ORAO_NETWORK_STATE, ORAO_VRF_PROGRAM_ID, TRIGGER_BOUNTY_LAMPORTS,
};
use crate::errors::EpochError;
use crate::events::EpochTransitionTriggered;
use crate::helpers::epoch_vrf_seed;
use crate::state::{ArbConfig, EpochState};

/// Accounts for trigger_epoch_transition instruction.
///
/// Initiates the VRF commit phase of an epoch transition.
/// The configured arb wallets have an exclusive trigger window; once the
/// transition is at least GRACE_SLOTS overdue, any signer may restore liveness.
#[derive(Accounts)]
pub struct TriggerEpochTransition<'info> {
    /// Payer who triggers the transition. Receives the trigger bounty.
    /// Must be wallet_a/wallet_b until the inclusive grace boundary opens.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Global epoch state singleton.
    /// Validated via seeds and bump.
    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
        constraint = epoch_state.safety_version == EPOCH_SAFETY_VERSION
            @ EpochError::SafetyActivationRequired,
    )]
    pub epoch_state: Account<'info, EpochState>,

    /// ArbConfig -- live-read epoch length and wallet gate.
    /// Hard-required after upgrade; the upgrade and initialization share one ceremony batch.
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump = arb_config.bump,
    )]
    pub arb_config: Account<'info, ArbConfig>,

    /// Carnage SOL vault PDA that funds the trigger bounty.
    /// The vault accrues 24% of all trade tax and has ample balance for bounties.
    /// Uses invoke_signed with PDA seeds to authorize the transfer.
    #[account(
        mut,
        seeds = [CARNAGE_SOL_VAULT_SEED],
        bump,
    )]
    pub carnage_sol_vault: SystemAccount<'info>,

    /// ORAO's network-configuration PDA. Names the fulfilment-authority set and
    /// carries the live `request_fee` and `treasury`; ORAO validates the passed
    /// treasury against it, so no fee or treasury value is compiled in here.
    #[account(
        mut,
        seeds = [CONFIG_ACCOUNT_SEED],
        bump,
        seeds::program = ORAO_VRF_PROGRAM_ID,
    )]
    pub network_state: Account<'info, NetworkState>,

    /// ORAO's fee treasury.
    /// CHECK: NOT pinned to a literal on purpose. ORAO enforces
    /// `network_state.config.treasury == treasury.key()`; a hardcoded value
    /// would silently break every request the day ORAO rotates it.
    #[account(mut)]
    pub treasury: AccountInfo<'info>,

    /// ORAO randomness request PDA for this (epoch, attempt).
    /// CHECK: ORAO `init`s this account inside the CPI below; we bind it to our
    /// own seed by re-derivation before the CPI, so a caller cannot substitute
    /// an account it controls.
    #[account(mut)]
    pub randomness_account: AccountInfo<'info>,

    /// The ORAO VRF program we CPI into.
    #[account(address = ORAO_VRF_PROGRAM_ID @ EpochError::InvalidRandomnessOwner)]
    pub vrf_program: Program<'info, OraoVrf>,

    pub system_program: Program<'info, System>,
}

// ============================================================================
// Helper Functions (public for unit testing)
// ============================================================================

/// Slot at which the current epoch becomes due.
///
/// Returns None on u64 overflow. ArbConfig bounds keep this unreachable for
/// valid live state, but the helper remains total for proof and edge testing.
pub fn epoch_due_slot(epoch_start_slot: u64, epoch_length_slots: u64) -> Option<u64> {
    epoch_start_slot.checked_add(epoch_length_slots)
}

/// Whether slot has reached the live-configured due boundary.
///
/// Overflow fails closed instead of panicking or admitting a transition.
pub fn is_epoch_due(slot: u64, epoch_start_slot: u64, epoch_length_slots: u64) -> bool {
    match epoch_due_slot(epoch_start_slot, epoch_length_slots) {
        Some(due_slot) => slot >= due_slot,
        None => false,
    }
}

/// Whether the permissionless grace window is open at slot.
///
/// The boundary is inclusive: a stranger is admitted exactly GRACE_SLOTS
/// after the epoch first becomes due. Overflow fails closed.
pub fn is_grace_open(slot: u64, epoch_start_slot: u64, epoch_length_slots: u64) -> bool {
    match epoch_due_slot(epoch_start_slot, epoch_length_slots)
        .and_then(|due_slot| due_slot.checked_add(GRACE_SLOTS))
    {
        Some(grace_slot) => slot >= grace_slot,
        None => false,
    }
}

// ============================================================================
// Instruction Handler
// ============================================================================

/// Trigger an epoch transition.
///
/// This is the first of TWO transactions in the ORAO VRF flow:
/// 1. TX 1: this instruction -- requests randomness via `request_v2` CPI
/// 2. ORAO's fulfilment nodes land their own transaction(s); we send nothing
/// 3. TX 2: consume_randomness
///
/// # Validations performed:
/// 1. Epoch boundary reached using the live ArbConfig length
/// 2. Payer is an arb wallet, or the inclusive grace window is open
/// 3. No VRF already pending (can't double-commit)
/// 4. The passed randomness account IS the PDA our seed derives to
///
/// Freshness and not-yet-revealed are no longer checked, and do not need to
/// be: the account did not exist before this instruction, because ORAO `init`s
/// it inside the CPI. `init` on an existing PDA fails and reverts the whole
/// trigger, so a stale or pre-revealed account cannot be smuggled in.
///
/// # State changes:
/// - Advances current_epoch by exactly one
/// - Re-anchors epoch_start_slot at the current slot
/// - Sets pause_end_slot to current slot plus the live pause length
/// - Sets vrf_request_slot to current slot
/// - Sets vrf_pending = true
/// - Sets taxes_confirmed = false
/// - Binds pending_randomness_account for anti-reroll protection
///
/// # Events:
/// Emits EpochTransitionTriggered
pub fn handler(ctx: Context<TriggerEpochTransition>) -> Result<()> {
    let epoch_state = &mut ctx.accounts.epoch_state;
    let clock = Clock::get()?;
    let epoch_length_slots = ctx.accounts.arb_config.epoch_length_slots;
    let payer = ctx.accounts.payer.key();

    // === 1. Validate epoch boundary reached ===
    require!(
        is_epoch_due(clock.slot, epoch_state.epoch_start_slot, epoch_length_slots),
        EpochError::EpochBoundaryNotReached
    );
    msg!(
        "Epoch boundary check: current_epoch={}, start_slot={}, length={}, slot={}",
        epoch_state.current_epoch,
        epoch_state.epoch_start_slot,
        epoch_length_slots,
        clock.slot
    );

    // === 2. Validate trigger admission before spending anything on ORAO ===
    require!(
        payer == ctx.accounts.arb_config.wallet_a
            || payer == ctx.accounts.arb_config.wallet_b
            || is_grace_open(clock.slot, epoch_state.epoch_start_slot, epoch_length_slots),
        EpochError::UnauthorizedTrigger
    );

    // === 3. Validate no VRF already pending ===
    // Prevents double-commit attacks and state inconsistency
    require!(!epoch_state.vrf_pending, EpochError::VrfAlreadyPending);
    require!(
        epoch_state.taxes_confirmed,
        EpochError::PreviousTransitionUnconfirmed
    );
    require!(
        !epoch_state.carnage_pending,
        EpochError::CarnagePendingBlocksTransition
    );

    // === 4. Request randomness from ORAO ===
    //
    // `next_epoch` is computed HERE, once, and reused by the epoch advance in
    // step 5. Deriving it twice would risk the seed and the stored epoch
    // drifting apart, after which consume could never re-derive the request.
    // Cutover continuity comes directly from the deployed current_epoch and
    // epoch_start_slot. No reset or migration is needed: even after a long
    // delay, one successful trigger advances one epoch and starts a full new one.
    let next_epoch = epoch_state
        .current_epoch
        .checked_add(1)
        .ok_or(EpochError::Overflow)?;

    // The first commitment of a generation is always attempt 1; retries bump it.
    let seed = epoch_vrf_seed(
        &epoch_state.key(),
        epoch_state.genesis_slot,
        next_epoch,
        1,
    );
    let expected_request =
        orao_solana_vrf::randomness_account_address(&ORAO_VRF_PROGRAM_ID, &seed);
    // Bind the passed account to OUR seed under ORAO's program before spending.
    // Together with ORAO owning the PDA this excludes foreign randomness: only
    // the ORAO program can write an account at this address.
    require_keys_eq!(
        ctx.accounts.randomness_account.key(),
        expected_request,
        EpochError::RandomnessAccountMismatch
    );

    orao_solana_vrf::cpi::request_v2(
        CpiContext::new(
            ctx.accounts.vrf_program.to_account_info(),
            orao_solana_vrf::cpi::accounts::RequestV2 {
                payer: ctx.accounts.payer.to_account_info(),
                network_state: ctx.accounts.network_state.to_account_info(),
                treasury: ctx.accounts.treasury.to_account_info(),
                request: ctx.accounts.randomness_account.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
        ),
        seed,
    )?;

    // === 5. Advance exactly once and re-anchor timing ===
    let pause_end_slot = clock
        .slot
        .checked_add(ctx.accounts.arb_config.pause_slots)
        .ok_or(EpochError::Overflow)?;
    epoch_state.current_epoch = next_epoch;
    epoch_state.epoch_start_slot = clock.slot;
    // A late/grace transition receives the full configured pause. Because the
    // epoch re-anchors here, this also equals pause from the new epoch start.
    epoch_state.pause_end_slot = pause_end_slot;

    // Complete the transition writes by setting the existing VRF pending state.
    epoch_state.vrf_request_slot = clock.slot;
    epoch_state.vrf_transition_start_slot = clock.slot;
    // A-05: this field's LAYOUT is frozen but its MEANING changed. It used to
    // snapshot the previous oracle's seed_slot; it now records the slot the
    // ORAO request was issued at. Kept for diagnostics and the timeout
    // arithmetic. The field name is deliberately NOT changed: EpochState's
    // layout is frozen (D-06) and four decoders read it at a fixed offset.
    epoch_state.pending_seed_slot = clock.slot;
    epoch_state.vrf_attempts = 1;
    epoch_state.vrf_pending = true;
    epoch_state.taxes_confirmed = false;

    // === 6. Bind randomness account (anti-reroll protection) ===
    // This is critical: consume_randomness MUST use the same account. Store the
    // DERIVED address; require_keys_eq! above already proved the passed account
    // equals it.
    epoch_state.pending_randomness_account = expected_request;
    msg!(
        "Bound randomness account: {} at slot {}",
        epoch_state.pending_randomness_account,
        clock.slot
    );

    // === 7. Pay bounty to triggerer from Carnage SOL vault ===
    // Reserve rent-exempt minimum so the vault PDA isn't garbage-collected.
    let rent = Rent::get()?;
    let rent_exempt_min = rent.minimum_balance(0);
    let vault_balance = ctx.accounts.carnage_sol_vault.lamports();
    let bounty_threshold = TRIGGER_BOUNTY_LAMPORTS
        .checked_add(rent_exempt_min)
        .ok_or(EpochError::Overflow)?;
    let bounty_paid = if vault_balance >= bounty_threshold {
        // Transfer bounty from carnage_sol_vault PDA to triggerer
        let vault_bump = ctx.bumps.carnage_sol_vault;
        let signer_seeds: &[&[u8]] = &[CARNAGE_SOL_VAULT_SEED, &[vault_bump]];

        invoke_signed(
            &system_instruction::transfer(
                ctx.accounts.carnage_sol_vault.to_account_info().key,
                ctx.accounts.payer.to_account_info().key,
                TRIGGER_BOUNTY_LAMPORTS,
            ),
            &[
                ctx.accounts.carnage_sol_vault.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[signer_seeds],
        )?;

        msg!(
            "Bounty paid: {} lamports from carnage_sol_vault to {}",
            TRIGGER_BOUNTY_LAMPORTS,
            ctx.accounts.payer.key()
        );
        TRIGGER_BOUNTY_LAMPORTS
    } else {
        msg!(
            "Carnage vault balance insufficient for bounty: {} < {} (skipped)",
            vault_balance,
            TRIGGER_BOUNTY_LAMPORTS
        );
        0
    };

    msg!(
        "Epoch {} triggered by {} at slot {} (bounty: {} lamports)",
        next_epoch,
        ctx.accounts.payer.key(),
        clock.slot,
        bounty_paid
    );

    // === 8. Emit event ===
    emit!(EpochTransitionTriggered {
        epoch: next_epoch,
        triggered_by: ctx.accounts.payer.key(),
        slot: clock.slot,
        bounty_paid,
        randomness_queue: ORAO_NETWORK_STATE,
        attempt: epoch_state.vrf_attempts,
    });

    Ok(())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_is_closed_one_slot_before_boundary() {
        let start = 1_000;
        let length = 200;
        assert!(!is_epoch_due(start + length - 1, start, length));
    }

    #[test]
    fn due_opens_at_boundary() {
        let start = 1_000;
        let length = 200;
        assert!(is_epoch_due(start + length, start, length));
    }

    #[test]
    fn due_remains_open_deep_past_boundary() {
        let start = 1_000;
        let length = 200;
        assert!(is_epoch_due(50_000, start, length));
    }

    #[test]
    fn grace_is_closed_at_299_overdue_slots() {
        let start = 1_000;
        let length = 200;
        assert!(!is_grace_open(
            start + length + GRACE_SLOTS - 1,
            start,
            length
        ));
    }

    #[test]
    fn grace_opens_at_inclusive_300_slot_boundary() {
        let start = 1_000;
        let length = 200;
        assert!(is_grace_open(start + length + GRACE_SLOTS, start, length));
    }

    #[test]
    fn grace_remains_open_after_boundary() {
        let start = 1_000;
        let length = 200;
        assert!(is_grace_open(
            start + length + GRACE_SLOTS + 1,
            start,
            length
        ));
    }

    #[test]
    fn grace_open_always_implies_due() {
        let start = 1_000;
        let length = 200;
        for slot in (0..=2_000).step_by(17) {
            if is_grace_open(slot, start, length) {
                assert!(is_epoch_due(slot, start, length));
            }
        }
    }

    #[test]
    fn live_read_length_moves_current_due_slot() {
        let start = 1_000;
        let slot = 1_500;
        assert!(is_epoch_due(slot, start, 100));
        assert!(!is_epoch_due(slot, start, 10_000));
    }

    #[test]
    fn shortening_length_can_make_current_epoch_instantly_due() {
        assert!(is_epoch_due(1_500, 1_000, 200));
    }

    #[test]
    fn shortening_length_can_make_grace_instantly_open() {
        // At slot 1_500, a live length of 150 makes the epoch 350 slots overdue.
        assert!(is_grace_open(1_500, 1_000, 150));
    }

    #[test]
    fn due_slot_overflow_returns_none() {
        assert_eq!(epoch_due_slot(u64::MAX, 216_000), None);
    }

    #[test]
    fn overflow_fails_due_and_grace_closed() {
        assert!(!is_epoch_due(u64::MAX, u64::MAX, 216_000));
        assert!(!is_grace_open(u64::MAX, u64::MAX, 216_000));

        // The due slot fits here, but adding GRACE_SLOTS does not.
        assert!(!is_grace_open(u64::MAX, u64::MAX - 200, 150));
    }

    #[test]
    fn helpers_are_total_at_zero_start() {
        assert!(!is_epoch_due(0, 0, 150));
        assert!(is_epoch_due(150, 0, 150));
    }

    #[test]
    fn due_slot_is_checked_sum() {
        assert_eq!(epoch_due_slot(1_000, 200), Some(1_200));
    }

    #[test]
    fn pause_gets_full_length_at_due_boundary() {
        let transition_slot = 1_200u64;
        let pause_slots = 75u64;
        let pause_end = transition_slot.checked_add(pause_slots).unwrap();
        assert_eq!(pause_end - transition_slot, pause_slots);
    }

    #[test]
    fn late_transition_still_gets_full_pause_length() {
        let transition_slot = 1_600u64;
        let pause_slots = 75u64;
        let pause_end = transition_slot.checked_add(pause_slots).unwrap();
        assert_eq!(pause_end - transition_slot, pause_slots);
    }
}
