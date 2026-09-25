//! expire_carnage instruction.
//!
//! Clears expired pending Carnage state after deadline passes.
//! SOL is retained in vault for next trigger.
//!
//! Source: Carnage_Fund_Spec.md Section 13.4

use anchor_lang::prelude::*;

use crate::constants::{
    CARNAGE_FUND_SEED, CARNAGE_SOL_VAULT_SEED, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED,
};
use crate::errors::EpochError;
use crate::events::{CarnageExpired, CarnageFailed};
use crate::state::{CarnageAction, CarnageFundState, EpochState};

/// Accounts for expire_carnage instruction.
///
/// This instruction clears expired pending Carnage state after the deadline.
/// Permissionless - anyone can call after deadline expires.
/// SOL is retained in vault for next Carnage trigger.
///
/// Source: Carnage_Fund_Spec.md Section 13.4
#[derive(Accounts)]
pub struct ExpireCarnage<'info> {
    /// Caller (anyone - permissionless)
    pub caller: Signer<'info>,

    /// Global epoch state (has pending Carnage flags)
    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
        constraint = epoch_state.safety_version == EPOCH_SAFETY_VERSION
            @ EpochError::SafetyActivationRequired,
        constraint = epoch_state.carnage_pending @ EpochError::NoCarnagePending,
    )]
    pub epoch_state: Account<'info, EpochState>,

    /// Carnage Fund state (read for sol_vault balance in event)
    #[account(
        seeds = [CARNAGE_FUND_SEED],
        bump = carnage_state.bump,
    )]
    pub carnage_state: Account<'info, CarnageFundState>,

    /// Carnage SOL vault (read for balance in event)
    /// CHECK: PDA derived from known seeds
    #[account(
        seeds = [CARNAGE_SOL_VAULT_SEED],
        bump,
    )]
    pub sol_vault: AccountInfo<'info>,
}

/// Handler for expire_carnage instruction.
///
/// Clears pending Carnage state after deadline expires.
/// SOL is retained in vault for next trigger.
///
/// # Errors
/// - `NoCarnagePending` if no Carnage is pending
/// - `CarnageDeadlineNotExpired` if deadline hasn't passed yet
///
/// Source: Carnage_Fund_Spec.md Section 13.4
pub fn handler(ctx: Context<ExpireCarnage>, generation: u32) -> Result<()> {
    let epoch_state = &mut ctx.accounts.epoch_state;
    let clock = Clock::get()?;

    require!(
        generation == epoch_state.current_epoch && generation == epoch_state.carnage_generation,
        EpochError::CarnageGenerationMismatch
    );

    // Validate deadline has passed
    require!(
        clock.slot > epoch_state.carnage_deadline_slot,
        EpochError::CarnageDeadlineNotExpired
    );

    msg!(
        "Carnage expired: current_slot={}, deadline={}, sol_retained={}",
        clock.slot,
        epoch_state.carnage_deadline_slot,
        ctx.accounts.sol_vault.lamports()
    );

    // Save values for event before clearing
    let expired_target = epoch_state.carnage_target;
    let expired_action = epoch_state.carnage_action;
    let deadline_slot = epoch_state.carnage_deadline_slot;
    let sol_retained = ctx.accounts.sol_vault.lamports();
    let current_epoch = epoch_state.current_epoch;

    // Clear pending state (SOL stays in vault)
    epoch_state.carnage_pending = false;
    epoch_state.carnage_action = CarnageAction::None.to_u8();
    epoch_state.carnage_generation = 0;
    epoch_state.carnage_deadline_slot = 0;
    epoch_state.carnage_lock_slot = 0;
    // Don't update last_carnage_epoch - Carnage didn't actually execute

    emit!(CarnageExpired {
        epoch: current_epoch,
        target: expired_target,
        action: expired_action,
        deadline_slot,
        sol_retained,
    });

    // Emit CarnageFailed for off-chain monitoring (Phase 47).
    // This is the definitive signal that both atomic and fallback paths failed.
    // The CarnageExpired event is kept for backward compatibility.
    // attempted_amount is 0 because we don't know what was attempted --
    // the failing transactions rolled back entirely.
    emit!(CarnageFailed {
        epoch: current_epoch,
        action: expired_action,
        target: expired_target,
        attempted_amount: 0, // Unknown: failing TXs rolled back
        vault_balance: sol_retained,
        slot: clock.slot,
        atomic: false, // Expiry means neither path succeeded
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_carnage_action_none_clearing() {
        // Verify CarnageAction::None converts to 0 for clearing
        assert_eq!(CarnageAction::None.to_u8(), 0);
    }

    #[test]
    fn test_deadline_comparison_logic() {
        // Test that > comparison works correctly for deadline expiration
        let deadline_slot: u64 = 100;
        let current_slot: u64 = 101;

        // current_slot > deadline_slot means deadline has passed
        assert!(current_slot > deadline_slot);

        // current_slot == deadline_slot means deadline has NOT passed yet
        let current_slot_at_deadline: u64 = 100;
        assert!(!(current_slot_at_deadline > deadline_slot));
    }
}
