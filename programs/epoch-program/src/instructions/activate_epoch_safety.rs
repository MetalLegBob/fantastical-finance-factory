//! One-way migration for the bounded VRF and generation-bound Carnage fields.
//!
//! The new fields are carved from EpochState's existing reserved region, so
//! the account size and all earlier offsets remain unchanged. Activation is an
//! explicit instruction—not an implicit reinterpretation—and is permitted only
//! while the old state machine is quiescent.

use anchor_lang::prelude::*;

use crate::constants::{EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED, MAX_VRF_ATTEMPTS};
use crate::errors::EpochError;
use crate::events::EpochSafetyActivated;
use crate::state::EpochState;

#[derive(Accounts)]
pub struct ActivateEpochSafety<'info> {
    /// Any signer may execute this deterministic liveness migration.
    pub caller: Signer<'info>,

    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
    )]
    pub epoch_state: Account<'info, EpochState>,
}

pub fn handler(ctx: Context<ActivateEpochSafety>) -> Result<()> {
    let state = &mut ctx.accounts.epoch_state;
    require!(
        state.safety_version == 0,
        EpochError::SafetyAlreadyActivated
    );
    require!(
        !state.carnage_pending,
        EpochError::SafetyActivationRequiresQuiescent
    );

    // 2026-09-19 (owner-directed minimal fix; Switchboard oracle shut down): an
    // inherited pre-safety VRF generation can never be revealed. Seed the bounded
    // generation so `terminal_vrf_fallback` becomes admissible (final-attempt
    // reason after VRF_TIMEOUT_SLOTS, absolute-age after MAX_VRF_PENDING_SLOTS)
    // and so `retry_epoch_vrf` refuses further foreign re-commits (attempts at
    // the maximum). A quiescent state activates exactly as before.
    let activation_slot = Clock::get()?.slot;
    if state.vrf_pending {
        state.vrf_transition_start_slot = activation_slot;
        state.vrf_attempts = MAX_VRF_ATTEMPTS;
    } else {
        state.vrf_transition_start_slot = 0;
        state.vrf_attempts = 0;
    }
    state.pending_seed_slot = 0;
    state.carnage_generation = 0;
    state.last_degraded_epoch = 0;
    state.last_vrf_fallback_reason = 0;
    state.reserved = [0u8; 28];
    state.safety_version = EPOCH_SAFETY_VERSION;

    emit!(EpochSafetyActivated {
        version: EPOCH_SAFETY_VERSION,
        slot: activation_slot,
        activated_by: ctx.accounts.caller.key(),
    });

    Ok(())
}
