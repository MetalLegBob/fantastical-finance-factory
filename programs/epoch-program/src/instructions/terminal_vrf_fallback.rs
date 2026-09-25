//! Deterministic terminal recovery for a bounded-out VRF transition.
//!
//! No caller chooses new economics here: the last confirmed tax tuple remains
//! active, Staking finalizes the epoch exactly as on successful consumption,
//! no Carnage action is scheduled, and the pending commitment is cleared.

use anchor_lang::prelude::*;

use crate::constants::{
    staking_program_id, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED, STAKING_AUTHORITY_SEED,
};
use crate::errors::EpochError;
use crate::events::VrfTerminalFallback;
use crate::helpers::{finalize_staking_epoch, terminal_vrf_reason};
use crate::state::{CarnageAction, EpochState};

#[derive(Accounts)]
pub struct TerminalVrfFallback<'info> {
    /// Anyone may restore liveness after an objective on-chain limit.
    pub caller: Signer<'info>,

    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
        constraint = epoch_state.safety_version == EPOCH_SAFETY_VERSION
            @ EpochError::SafetyActivationRequired,
    )]
    pub epoch_state: Account<'info, EpochState>,

    /// Epoch Program PDA authorized by Staking.
    /// CHECK: canonical PDA derivation is enforced here and by Staking.
    #[account(seeds = [STAKING_AUTHORITY_SEED], bump)]
    pub staking_authority: AccountInfo<'info>,

    /// Staking singleton updated by the canonical Staking program.
    /// CHECK: Staking validates its own account discriminator and PDA.
    #[account(mut)]
    pub stake_pool: AccountInfo<'info>,

    /// Canonical Staking program.
    /// CHECK: exact cluster address is enforced.
    #[account(address = staking_program_id() @ EpochError::InvalidStakingProgram)]
    pub staking_program: AccountInfo<'info>,
}

pub fn handler(ctx: Context<TerminalVrfFallback>) -> Result<()> {
    let state = &mut ctx.accounts.epoch_state;
    let clock = Clock::get()?;

    require!(state.vrf_pending, EpochError::NoVrfPending);
    require!(state.vrf_attempts > 0, EpochError::InvalidEpochState);
    require!(
        !state.carnage_pending,
        EpochError::CarnagePendingBlocksTransition
    );

    let reason = terminal_vrf_reason(
        clock.slot,
        state.vrf_transition_start_slot,
        state.vrf_request_slot,
        state.vrf_attempts,
    )
    .ok_or(EpochError::VrfTerminalLimitNotReached)?;

    let epoch = state.current_epoch;
    let attempts = state.vrf_attempts;
    let transition_start_slot = state.vrf_transition_start_slot;

    // Match the successful consume path's epoch finalization. Any CPI failure
    // rolls back the state clear and leaves recovery retryable.
    finalize_staking_epoch(
        &ctx.accounts.staking_authority,
        &ctx.accounts.stake_pool,
        &ctx.accounts.staking_program,
        ctx.bumps.staking_authority,
        epoch,
    )?;

    // Retain the prior confirmed tax tuple; only resolve lifecycle metadata.
    state.vrf_pending = false;
    state.taxes_confirmed = true;
    state.vrf_request_slot = 0;
    state.pending_randomness_account = Pubkey::default();
    state.pending_seed_slot = 0;
    state.last_degraded_epoch = epoch;
    state.last_vrf_fallback_reason = reason;

    // A failed randomness generation cannot schedule a random consequence.
    state.carnage_pending = false;
    state.carnage_generation = 0;
    state.carnage_action = CarnageAction::None.to_u8();
    state.carnage_deadline_slot = 0;
    state.carnage_lock_slot = 0;

    emit!(VrfTerminalFallback {
        epoch,
        reason,
        attempts,
        transition_start_slot,
        resolved_slot: clock.slot,
        cheap_side: state.cheap_side,
        crime_buy_tax_bps: state.crime_buy_tax_bps,
        crime_sell_tax_bps: state.crime_sell_tax_bps,
        fraud_buy_tax_bps: state.fraud_buy_tax_bps,
        fraud_sell_tax_bps: state.fraud_sell_tax_bps,
        pause_end_slot: state.pause_end_slot,
        staking_finalized: true,
    });

    Ok(())
}
