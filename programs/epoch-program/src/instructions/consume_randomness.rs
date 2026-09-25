//! consume_randomness instruction.
//!
//! Reads the fulfilled ORAO randomness, verifies anti-reroll protection,
//! derives new tax rates, and updates EpochState.
//!
//! This is the SECOND and last of our two transactions. Nothing is bundled:
//! ORAO's fulfilment nodes land their own transaction, so by the time this
//! runs the request PDA has already been rewritten in place as Fulfilled.
//! Source: Epoch_State_Machine_Spec.md Section 8.3; 179.1-RESEARCH.md Pattern 3

use anchor_lang::prelude::*;
use orao_solana_vrf::state::{RandomnessAccountData, RandomnessAccountVersion};

use crate::constants::{
    staking_program_id, CARNAGE_DEADLINE_SLOTS, CARNAGE_FUND_SEED, CARNAGE_LOCK_SLOTS,
    EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED, ORAO_VRF_PROGRAM_ID, STAKING_AUTHORITY_SEED,
};
use crate::errors::EpochError;
use crate::events::{CarnageNotTriggered, CarnagePending, TaxesUpdated};
use crate::helpers::{
    derive_taxes, epoch_vrf_seed, finalize_staking_epoch, get_carnage_action, get_carnage_target,
    is_carnage_triggered, terminal_vrf_reason,
};
use crate::state::{CarnageAction, CarnageFundState, EpochState, Token};

/// Minimum VRF bytes needed for tax derivation + Carnage.
/// Bytes 0-4: Tax (flip + 4 independent magnitude rolls)
/// Bytes 5-7: Carnage (trigger + action + target)
/// Total: 8 bytes of 32 available.
/// Source: Epoch_State_Machine_Spec.md Section 7.2 (updated Phase 37)
pub const MIN_VRF_BYTES: usize = 8;

/// Accounts for the consume_randomness instruction.
///
/// Called after ORAO's fulfilment nodes have written the randomness (observed
/// 1-3 s on devnet, ~36 s on mainnet). Verifies anti-reroll protection, reads
/// the VRF bytes, derives tax rates. Nothing needs bundling: ORAO fulfils in
/// its own transaction.
#[derive(Accounts)]
pub struct ConsumeRandomness<'info> {
    /// Caller (anyone can call after oracle reveals).
    pub caller: Signer<'info>,

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

    /// ORAO randomness request account (MUST match pending_randomness_account).
    /// CHECK: owner-validated against ORAO here, then bound twice more in the
    /// handler -- by the stored pending key and by seed re-derivation.
    #[account(owner = ORAO_VRF_PROGRAM_ID @ EpochError::InvalidRandomnessOwner)]
    pub randomness_account: AccountInfo<'info>,

    /// Staking authority PDA - Epoch Program signs CPIs to Staking.
    /// CHECK: PDA derived from this program's seeds, validated by seeds constraint.
    #[account(
        seeds = [STAKING_AUTHORITY_SEED],
        bump,
    )]
    pub staking_authority: AccountInfo<'info>,

    /// Staking Program's pool state (mutable for update_cumulative).
    /// CHECK: Validated by Staking Program during CPI.
    #[account(mut)]
    pub stake_pool: AccountInfo<'info>,

    /// Staking Program for update_cumulative CPI.
    /// CHECK: Address validated against known Staking program ID.
    #[account(address = staking_program_id() @ EpochError::InvalidStakingProgram)]
    pub staking_program: AccountInfo<'info>,

    /// Canonical Carnage Fund state. This consequence account is mandatory:
    /// callers cannot commit new taxes while omitting Carnage processing.
    #[account(
        seeds = [CARNAGE_FUND_SEED],
        bump = carnage_state.bump,
        constraint = carnage_state.initialized @ EpochError::CarnageNotInitialized,
    )]
    pub carnage_state: Account<'info, CarnageFundState>,
}

/// Handler for consume_randomness instruction.
///
/// # Flow
/// 0. Auto-expire stale pending Carnage (if deadline passed)
/// 1. Validate VRF is pending
/// 2. Anti-reroll: verify SAME randomness account that was committed
/// 3. Re-derive the request seed and read the fulfilled randomness
/// 4. Validate sufficient bytes (MIN_VRF_BYTES = 8)
/// 5. Derive tax rates from VRF bytes
/// 6. Update EpochState with new tax configuration
/// 7. Clear VRF pending state
/// 7.5. CPI to Staking: finalize epoch yield
/// 8. Emit TaxesUpdated event
/// 9. Carnage trigger check (if carnage_state provided):
///    - Check if VRF byte 5 < 11 (triggers Carnage)
///    - If triggered: set pending state for execute_carnage_atomic
///    - Emit CarnagePending or CarnageNotTriggered event
///
/// # Errors
/// - `NoVrfPending` if no VRF request is pending
/// - `RandomnessAccountMismatch` if account doesn't match bound account (anti-reroll)
/// - `RandomnessParseError` if randomness account data is invalid
/// - `RandomnessNotRevealed` if oracle hasn't revealed yet
/// - `InsufficientRandomness` if less than 8 bytes revealed
/// - `Overflow` if deadline slot calculation overflows
pub fn handler(ctx: Context<ConsumeRandomness>) -> Result<()> {
    let epoch_state = &mut ctx.accounts.epoch_state;
    let clock = Clock::get()?;

    // === 1. Validate VRF is pending ===
    require!(epoch_state.vrf_pending, EpochError::NoVrfPending);
    require!(epoch_state.vrf_attempts > 0, EpochError::InvalidEpochState);
    require!(
        terminal_vrf_reason(
            clock.slot,
            epoch_state.vrf_transition_start_slot,
            epoch_state.vrf_request_slot,
            epoch_state.vrf_attempts,
        )
        .is_none(),
        EpochError::VrfTerminalTransitionRequired
    );

    // === 2. Anti-reroll: verify SAME randomness account that was committed ===
    require!(
        ctx.accounts.randomness_account.key() == epoch_state.pending_randomness_account,
        EpochError::RandomnessAccountMismatch
    );
    msg!(
        "Anti-reroll verified: {} matches bound account",
        ctx.accounts.randomness_account.key()
    );

    // === 3. Read the fulfilled ORAO randomness ===
    //
    // The seed is re-derived from the SAME (epoch, attempt) the request was
    // made under. This replaces the old seed-slot equality check (A-05) and is
    // the third independent bind on this account: ORAO owns it (accounts
    // constraint), it is the key we stored at trigger/retry (step 2), and it
    // is the PDA our seed derives to (the seed equality below).
    let seed = epoch_vrf_seed(
        &epoch_state.key(),
        epoch_state.genesis_slot,
        epoch_state.current_epoch,
        epoch_state.vrf_attempts,
    );
    let vrf_result: [u8; 32] = {
        let data = ctx.accounts.randomness_account.try_borrow_data()?;
        let parsed = RandomnessAccountData::try_deserialize(&mut &data[..])
            .map_err(|_| EpochError::RandomnessParseError)?;
        require!(
            parsed.version() == RandomnessAccountVersion::V2,
            EpochError::RandomnessParseError
        );
        require!(
            parsed.seed() == &seed,
            EpochError::RandomnessCommitmentMismatch
        );
        let full: &[u8; 64] = parsed
            .fulfilled_randomness()
            .ok_or(EpochError::RandomnessNotRevealed)?;

        // TAKE THE FIRST 32 BYTES. NEVER THE UPPER HALF. NEVER THE LAST BYTE.
        //
        // ORAO's 64-byte value is an XOR of ed25519 signatures. An ed25519
        // signature is R || S with S < l ~= 2^252, so the top bits of the tail
        // are almost always zero and SURVIVE the XOR -- both live samples show
        // it (mainnet byte 63 = 0x0c, devnet = 0x02). Bytes 0..32 come from R,
        // a curve-point encoding, and are uniform.
        //
        // derive_taxes / is_carnage_triggered / get_carnage_action /
        // get_carnage_target read bytes 0-7, i.e. exactly this safe region.
        // Using the tail would silently bias every tax and carnage roll while
        // still looking random. This is the single easiest line in the program
        // for a future reader to "tidy" into a bug.
        full[..32]
            .try_into()
            .map_err(|_| EpochError::InsufficientRandomness)?
    };

    // === 4. Validate sufficient bytes ===
    // Note: the slice above is [u8; 32], so this is always satisfied,
    // but we keep the check for defensive programming and documentation.
    require!(
        vrf_result.len() >= MIN_VRF_BYTES,
        EpochError::InsufficientRandomness
    );
    msg!(
        "VRF bytes received: [{}, {}, {}, {}, {}, {}, {}, {}]",
        vrf_result[0],
        vrf_result[1],
        vrf_result[2],
        vrf_result[3],
        vrf_result[4],
        vrf_result[5],
        vrf_result[6],
        vrf_result[7]
    );

    // === 5. Derive tax rates from VRF bytes ===
    let old_cheap_side = epoch_state.cheap_side;
    let current_token =
        Token::from_u8(epoch_state.cheap_side).ok_or(EpochError::InvalidCheapSide)?;
    let tax_config = derive_taxes(&vrf_result, current_token);

    // === 6. Update EpochState with new tax configuration ===
    epoch_state.cheap_side = tax_config.cheap_side.to_u8();
    epoch_state.crime_buy_tax_bps = tax_config.crime_buy_tax_bps;
    epoch_state.crime_sell_tax_bps = tax_config.crime_sell_tax_bps;
    epoch_state.fraud_buy_tax_bps = tax_config.fraud_buy_tax_bps;
    epoch_state.fraud_sell_tax_bps = tax_config.fraud_sell_tax_bps;

    // VRF-03: Populate legacy summary fields with min/max of per-token rates.
    // These fields are kept for event emission and external consumers (e.g. UI).
    // derive_taxes() returns 0 for these (rates are independent per-token),
    // so we compute min/max explicitly from the 4 per-token rates.
    epoch_state.low_tax_bps = tax_config
        .crime_buy_tax_bps
        .min(tax_config.crime_sell_tax_bps)
        .min(tax_config.fraud_buy_tax_bps)
        .min(tax_config.fraud_sell_tax_bps);
    epoch_state.high_tax_bps = tax_config
        .crime_buy_tax_bps
        .max(tax_config.crime_sell_tax_bps)
        .max(tax_config.fraud_buy_tax_bps)
        .max(tax_config.fraud_sell_tax_bps);

    // === 7. Clear VRF pending state ===
    epoch_state.vrf_pending = false;
    epoch_state.taxes_confirmed = true;
    epoch_state.vrf_request_slot = 0;
    epoch_state.pending_randomness_account = Pubkey::default();
    epoch_state.pending_seed_slot = 0;

    let flipped = epoch_state.cheap_side != old_cheap_side;
    msg!(
        "Taxes updated: cheap_side={} (flipped={}), low={}, high={}",
        epoch_state.cheap_side,
        flipped,
        epoch_state.low_tax_bps,
        epoch_state.high_tax_bps
    );

    // === 7.5. CPI TO STAKING: FINALIZE EPOCH YIELD ===
    // Notify staking that the epoch has advanced and yield is finalized.
    // This happens AFTER tax derivation, BEFORE Carnage check (per CONTEXT.md).
    // Timing: validate randomness -> derive new rates -> finalize old epoch yield -> check Carnage
    finalize_staking_epoch(
        &ctx.accounts.staking_authority,
        &ctx.accounts.stake_pool,
        &ctx.accounts.staking_program,
        ctx.bumps.staking_authority,
        epoch_state.current_epoch,
    )?;

    msg!(
        "Staking cumulative updated for epoch {}",
        epoch_state.current_epoch
    );

    // === 8. Emit event ===
    emit!(TaxesUpdated {
        epoch: epoch_state.current_epoch,
        cheap_side: epoch_state.cheap_side,
        low_tax_bps: epoch_state.low_tax_bps,
        high_tax_bps: epoch_state.high_tax_bps,
        flipped,
    });

    // === 9. Mandatory Carnage trigger check ===
    if is_carnage_triggered(&vrf_result) {
        let has_holdings = ctx.accounts.carnage_state.held_amount > 0;
        let action = get_carnage_action(&vrf_result, has_holdings);
        let target = get_carnage_target(&vrf_result);

        epoch_state.carnage_pending = true;
        epoch_state.carnage_generation = epoch_state.current_epoch;
        epoch_state.carnage_action = action.to_u8();
        epoch_state.carnage_target = target.to_u8();
        epoch_state.carnage_deadline_slot = clock
            .slot
            .checked_add(CARNAGE_DEADLINE_SLOTS)
            .ok_or(EpochError::Overflow)?;
        epoch_state.carnage_lock_slot = clock
            .slot
            .checked_add(CARNAGE_LOCK_SLOTS)
            .ok_or(EpochError::Overflow)?;

        emit!(CarnagePending {
            epoch: epoch_state.carnage_generation,
            target: target.to_u8(),
            action: action.to_u8(),
            deadline_slot: epoch_state.carnage_deadline_slot,
        });
    } else {
        epoch_state.carnage_pending = false;
        epoch_state.carnage_generation = 0;
        epoch_state.carnage_action = CarnageAction::None.to_u8();
        epoch_state.carnage_deadline_slot = 0;
        epoch_state.carnage_lock_slot = 0;

        emit!(CarnageNotTriggered {
            epoch: epoch_state.current_epoch,
            vrf_byte: vrf_result[5],
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::CARNAGE_DEADLINE_SLOTS;

    #[test]
    fn test_min_vrf_bytes_constant() {
        // Per spec Section 7.2 (updated Phase 37), we need 8 bytes:
        // 0: flip, 1: crime_low, 2: crime_high, 3: fraud_low, 4: fraud_high,
        // 5: carnage_trigger, 6: carnage_action, 7: carnage_target
        assert_eq!(MIN_VRF_BYTES, 8);
    }

    #[test]
    fn test_vrf_bytes_are_sufficient() {
        // ORAO provides 64 bytes, of which we take the safe first 32 and
        // only need 8
        let vrf_result = vec![0u8; 32];
        assert!(vrf_result.len() >= MIN_VRF_BYTES);
    }

    // === Carnage integration tests ===

    #[test]
    fn test_carnage_trigger_logic_integration() {
        // Simulate a VRF result that triggers Carnage (byte 5)
        let mut vrf = [0u8; 32];
        vrf[5] = 5; // < 11 = trigger

        assert!(is_carnage_triggered(&vrf));

        // With no holdings
        let action = get_carnage_action(&vrf, false);
        assert_eq!(action, CarnageAction::None);

        // With holdings and byte 6 < 5 = sell
        vrf[6] = 3;
        let action = get_carnage_action(&vrf, true);
        assert_eq!(action, CarnageAction::Sell);

        // With holdings and byte 6 >= 5 = burn
        vrf[6] = 100;
        let action = get_carnage_action(&vrf, true);
        assert_eq!(action, CarnageAction::Burn);
    }

    #[test]
    fn test_carnage_no_trigger_integration() {
        // Simulate a VRF result that doesn't trigger Carnage (byte 5)
        let mut vrf = [0u8; 32];
        vrf[5] = 11; // >= 11 = no trigger

        assert!(!is_carnage_triggered(&vrf));
    }

    #[test]
    fn test_carnage_target_integration() {
        let mut vrf = [0u8; 32];

        // byte 7 < 128 = CRIME
        vrf[7] = 50;
        assert_eq!(get_carnage_target(&vrf), Token::Crime);

        // byte 7 >= 128 = FRAUD
        vrf[7] = 200;
        assert_eq!(get_carnage_target(&vrf), Token::Fraud);
    }

    #[test]
    fn test_deadline_calculation() {
        // Verify deadline is correctly calculated
        let current_slot: u64 = 1000;
        let deadline = current_slot.checked_add(CARNAGE_DEADLINE_SLOTS).unwrap();
        assert_eq!(deadline, 1300); // 1000 + 300 = 1300
    }

    #[test]
    fn test_stale_pending_detection() {
        // Simulate stale pending scenario
        let deadline_slot: u64 = 1000;
        let current_slot: u64 = 1001;

        // Stale: current > deadline
        assert!(current_slot > deadline_slot);
    }

    #[test]
    fn test_valid_pending_detection() {
        // Simulate valid pending scenario
        let deadline_slot: u64 = 1000;
        let current_slot: u64 = 999;

        // Valid: current <= deadline
        assert!(current_slot <= deadline_slot);
    }

    #[test]
    fn test_carnage_action_none_u8_value() {
        // Verify CarnageAction::None.to_u8() is 0 for state clearing
        assert_eq!(CarnageAction::None.to_u8(), 0);
    }
}
