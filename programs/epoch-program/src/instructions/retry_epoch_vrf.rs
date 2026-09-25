//! retry_epoch_vrf instruction.
//!
//! Requests a FRESH ORAO randomness account after a VRF timeout.
//! Prevents protocol deadlock if the oracle fails to fulfil.
//!
//! Like `trigger_epoch_transition`, this issues an `orao_vrf::request_v2` CPI
//! itself: there is no client-side account creation and no SDK commit
//! instruction to bundle. The replacement request is addressed by
//! `epoch_vrf_seed(.., current_epoch, vrf_attempts + 1)`, so bumping the
//! attempt is what makes it a genuinely new PDA.
//!
//! This instruction changes the randomness PROVIDER only. The retry
//! admissibility arithmetic and the `vrf_attempts` ladder below are the
//! bounded-VRF policy and are preserved exactly (D-03).
//! Source: Epoch_State_Machine_Spec.md Section 8.6; 179.1-RESEARCH.md Pattern 1

use anchor_lang::prelude::*;
use orao_solana_vrf::program::OraoVrf;
use orao_solana_vrf::state::NetworkState;
use orao_solana_vrf::CONFIG_ACCOUNT_SEED;

use crate::constants::{
    ARB_CONFIG_SEED, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED, GRACE_SLOTS, MAX_VRF_ATTEMPTS,
    MAX_VRF_PENDING_SLOTS, ORAO_NETWORK_STATE, ORAO_VRF_PROGRAM_ID, VRF_TIMEOUT_SLOTS,
};
use crate::errors::EpochError;
use crate::events::VrfRetryRequested;
use crate::helpers::epoch_vrf_seed;
use crate::state::{ArbConfig, EpochState};

/// Accounts for retry_epoch_vrf instruction.
///
/// Allows re-committing a new randomness account after VRF timeout. The
/// configured keepers receive a bounded preference window, after which any
/// signer may restore liveness.
#[derive(Accounts)]
pub struct RetryEpochVrf<'info> {
    /// Payer for the retry. Must be wallet A/B until public recovery opens.
    /// Writable because the ORAO CPI debits it for the request fee and the
    /// request PDA's rent.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Global epoch state.
    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
        constraint = epoch_state.safety_version == EPOCH_SAFETY_VERSION
            @ EpochError::SafetyActivationRequired,
    )]
    pub epoch_state: Account<'info, EpochState>,

    /// Canonical keeper roster. Wallet A/B get the first recovery window;
    /// public callers are admitted only after that bounded preference expires.
    #[account(seeds = [ARB_CONFIG_SEED], bump = arb_config.bump)]
    pub arb_config: Account<'info, ArbConfig>,

    /// ORAO's network-configuration PDA. See `TriggerEpochTransition`.
    #[account(
        mut,
        seeds = [CONFIG_ACCOUNT_SEED],
        bump,
        seeds::program = ORAO_VRF_PROGRAM_ID,
    )]
    pub network_state: Account<'info, NetworkState>,

    /// ORAO's fee treasury.
    /// CHECK: NOT pinned to a literal on purpose. ORAO enforces
    /// `network_state.config.treasury == treasury.key()`.
    #[account(mut)]
    pub treasury: AccountInfo<'info>,

    /// Fresh ORAO randomness request PDA for (current_epoch, vrf_attempts + 1).
    /// CHECK: ORAO `init`s this account inside the CPI below; we bind it to our
    /// own seed by re-derivation before the CPI.
    #[account(mut)]
    pub randomness_account: AccountInfo<'info>,

    /// The ORAO VRF program we CPI into.
    #[account(address = ORAO_VRF_PROGRAM_ID @ EpochError::InvalidRandomnessOwner)]
    pub vrf_program: Program<'info, OraoVrf>,

    /// Required by the ORAO CPI, which creates an account. This instruction did
    /// not previously declare it.
    pub system_program: Program<'info, System>,
}

/// A public caller may select a replacement only after the oracle timeout and
/// one additional keeper grace window. The strict `>` boundary preserves the
/// existing retry rule: a timeout becomes actionable one slot after it ends.
pub fn is_public_retry_open(current_slot: u64, request_slot: u64) -> bool {
    request_slot
        .checked_add(VRF_TIMEOUT_SLOTS)
        .and_then(|slot| slot.checked_add(GRACE_SLOTS))
        .map(|public_boundary| current_slot > public_boundary)
        .unwrap_or(false)
}

/// A replacement is admitted only if it can receive the complete reveal
/// window before the inclusive absolute-age fallback boundary.
pub fn retry_preserves_full_reveal_window(current_slot: u64, transition_start_slot: u64) -> bool {
    current_slot
        .checked_add(VRF_TIMEOUT_SLOTS)
        .zip(transition_start_slot.checked_add(MAX_VRF_PENDING_SLOTS))
        .map(|(reveal_window_end, absolute_boundary)| reveal_window_end < absolute_boundary)
        .unwrap_or(false)
}

/// Handler for retry_epoch_vrf instruction.
///
/// # Flow
/// 1. Validate VRF is pending
/// 2. Validate timeout has elapsed (elapsed_slots > VRF_TIMEOUT_SLOTS)
/// 3. Derive the attempt+1 seed, bind the request PDA, request via ORAO CPI
/// 4. Overwrite pending state with the new randomness account
/// 5. Emit VrfRetryRequested event
///
/// Freshness and not-yet-revealed checks are gone because they are inherent:
/// ORAO `init`s the request inside the CPI, so it cannot be stale or already
/// fulfilled.
///
/// # Errors
/// - `NoVrfPending` if no VRF request is pending
/// - `VrfTimeoutNotElapsed` if 300 slots haven't passed since original request
/// - `RandomnessAccountMismatch` if the passed account is not the PDA our seed
///   derives to
/// - `RandomnessReplacementUnchanged` if the replacement equals the pending one
pub fn handler(ctx: Context<RetryEpochVrf>) -> Result<()> {
    let epoch_state = &mut ctx.accounts.epoch_state;
    let clock = Clock::get()?;

    // === 1. Validate VRF is pending ===
    require!(epoch_state.vrf_pending, EpochError::NoVrfPending);
    require!(epoch_state.vrf_attempts > 0, EpochError::InvalidEpochState);

    // === 2. Validate timeout has elapsed ===
    let elapsed_slots = clock.slot.saturating_sub(epoch_state.vrf_request_slot);
    msg!(
        "VRF timeout check: elapsed={} slots, timeout={} slots",
        elapsed_slots,
        VRF_TIMEOUT_SLOTS
    );
    require!(
        elapsed_slots > VRF_TIMEOUT_SLOTS,
        EpochError::VrfTimeoutNotElapsed
    );

    let payer = ctx.accounts.payer.key();
    require!(
        payer == ctx.accounts.arb_config.wallet_a
            || payer == ctx.accounts.arb_config.wallet_b
            || is_public_retry_open(clock.slot, epoch_state.vrf_request_slot),
        EpochError::UnauthorizedVrfRetry
    );

    // Retry cannot reset either the cumulative commitment budget or the
    // immutable transition lifetime.
    require!(
        epoch_state.vrf_attempts < MAX_VRF_ATTEMPTS,
        EpochError::VrfAttemptLimitReached
    );
    let absolute_age = clock
        .slot
        .saturating_sub(epoch_state.vrf_transition_start_slot);
    require!(
        absolute_age < MAX_VRF_PENDING_SLOTS,
        EpochError::VrfAbsoluteAgeLimitReached
    );
    require!(
        retry_preserves_full_reveal_window(clock.slot, epoch_state.vrf_transition_start_slot),
        EpochError::VrfAbsoluteAgeLimitReached
    );
    // Still meaningful under ORAO: attempt+1 derives a DIFFERENT PDA, so this
    // now proves the retry actually moved on rather than re-submitting the
    // same request.
    require_keys_neq!(
        ctx.accounts.randomness_account.key(),
        epoch_state.pending_randomness_account,
        EpochError::RandomnessReplacementUnchanged
    );

    // === 3. Request a FRESH randomness account from ORAO ===
    //
    // The attempt bump is what makes the PDA new: the same (epoch, attempt)
    // would re-derive the existing account and ORAO's `init` would revert.
    // Computed once here and reused by the bookkeeping in step 4, so the seed
    // and the stored attempt cannot drift apart.
    let next_attempt = epoch_state
        .vrf_attempts
        .checked_add(1)
        .ok_or(EpochError::Overflow)?;

    let seed = epoch_vrf_seed(
        &epoch_state.key(),
        epoch_state.genesis_slot,
        epoch_state.current_epoch,
        next_attempt,
    );
    let expected_request =
        orao_solana_vrf::randomness_account_address(&ORAO_VRF_PROGRAM_ID, &seed);
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

    // === 4. Overwrite pending state with new randomness account ===
    let original_slot = epoch_state.vrf_request_slot;
    let original_account = epoch_state.pending_randomness_account;

    epoch_state.vrf_request_slot = clock.slot;
    epoch_state.pending_randomness_account = expected_request;
    // A-05: the request slot, not an oracle commitment snapshot. Layout frozen.
    epoch_state.pending_seed_slot = clock.slot;
    epoch_state.vrf_attempts = next_attempt;

    msg!(
        "VRF retry: replaced {} (slot {}) with {} (slot {})",
        original_account,
        original_slot,
        epoch_state.pending_randomness_account,
        clock.slot
    );

    // === 5. Emit event ===
    emit!(VrfRetryRequested {
        epoch: epoch_state.current_epoch,
        original_request_slot: original_slot,
        retry_slot: clock.slot,
        requested_by: ctx.accounts.payer.key(),
        transition_start_slot: epoch_state.vrf_transition_start_slot,
        attempt: epoch_state.vrf_attempts,
        randomness_queue: ORAO_NETWORK_STATE,
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrf_timeout_slots_constant() {
        // Per spec Section 3.1, VRF timeout is 300 slots (~2 minutes)
        assert_eq!(VRF_TIMEOUT_SLOTS, 300);
    }

    #[test]
    fn test_timeout_boundary_logic() {
        // Simulate the timeout check logic used in the handler
        let vrf_request_slot: u64 = 1000;

        // At exactly timeout: NOT allowed (need > not >=)
        let current_slot: u64 = 1300; // 300 slots elapsed
        let elapsed = current_slot.saturating_sub(vrf_request_slot);
        assert_eq!(elapsed, 300);
        assert!(
            !(elapsed > VRF_TIMEOUT_SLOTS),
            "Should NOT allow retry at exactly 300 slots"
        );

        // One slot after timeout: allowed
        let current_slot: u64 = 1301; // 301 slots elapsed
        let elapsed = current_slot.saturating_sub(vrf_request_slot);
        assert_eq!(elapsed, 301);
        assert!(
            elapsed > VRF_TIMEOUT_SLOTS,
            "Should allow retry at 301 slots"
        );

        // Well after timeout: allowed
        let current_slot: u64 = 2000; // 1000 slots elapsed
        let elapsed = current_slot.saturating_sub(vrf_request_slot);
        assert!(
            elapsed > VRF_TIMEOUT_SLOTS,
            "Should allow retry at 1000 slots"
        );
    }

    #[test]
    fn test_saturating_sub_handles_underflow() {
        // Edge case: current_slot < vrf_request_slot (shouldn't happen but be safe)
        let vrf_request_slot = 1000u64;
        let current_slot = 500u64;
        let elapsed = current_slot.saturating_sub(vrf_request_slot);
        assert_eq!(elapsed, 0, "saturating_sub should return 0 on underflow");
        assert!(
            !(elapsed > VRF_TIMEOUT_SLOTS),
            "Underflow case should NOT allow retry"
        );
    }

    #[test]
    fn public_retry_boundary_follows_the_keeper_grace_window() {
        let request_slot = 1_000;
        assert!(!is_public_retry_open(
            request_slot + VRF_TIMEOUT_SLOTS + GRACE_SLOTS,
            request_slot,
        ));
        assert!(is_public_retry_open(
            request_slot + VRF_TIMEOUT_SLOTS + GRACE_SLOTS + 1,
            request_slot,
        ));
        assert!(!is_public_retry_open(u64::MAX, u64::MAX));
    }

    #[test]
    fn retries_cannot_create_an_attempt_with_a_truncated_reveal_window() {
        let start = 1_000;
        let absolute_boundary = start + MAX_VRF_PENDING_SLOTS;
        assert!(retry_preserves_full_reveal_window(
            absolute_boundary - VRF_TIMEOUT_SLOTS - 1,
            start,
        ));
        assert!(!retry_preserves_full_reveal_window(
            absolute_boundary - VRF_TIMEOUT_SLOTS,
            start,
        ));
        assert!(!retry_preserves_full_reveal_window(u64::MAX, start));
    }
}
