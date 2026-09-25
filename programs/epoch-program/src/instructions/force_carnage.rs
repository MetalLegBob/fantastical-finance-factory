//! force_carnage instruction (DEVNET ONLY).
//!
//! Admin-gated test helper that sets carnage_pending on EpochState
//! without waiting for a natural VRF trigger. Allows rapid testing
//! of all Carnage execution paths (Burn, Sell, BuyOnly).
//!
//! MUST BE REMOVED BEFORE MAINNET DEPLOYMENT.
//! Gated by #[cfg(feature = "devnet")] at module and instruction level.

use anchor_lang::prelude::*;
use anchor_lang::pubkey;

use crate::constants::{
    CARNAGE_DEADLINE_SLOTS, CARNAGE_LOCK_SLOTS, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED,
};
use crate::errors::EpochError;
use crate::state::EpochState;

/// Devnet deployer wallet - the only account allowed to force Carnage.
/// This is the wallet at keypairs/devnet-wallet.json.
const DEVNET_ADMIN: Pubkey = pubkey!("8kPzhQoUPx7LYM18f9TzskW4ZgvGyq4jMPYZikqmHMH4");

/// Accounts for the force_carnage instruction.
///
/// Admin-only: requires the deployer wallet to sign.
#[derive(Accounts)]
pub struct ForceCarnage<'info> {
    /// Admin signer (must be the devnet deployer wallet).
    #[account(
        constraint = authority.key() == DEVNET_ADMIN @ EpochError::NotInitialized,
    )]
    pub authority: Signer<'info>,

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
}

/// Handler for force_carnage instruction.
///
/// Sets the carnage_pending state on EpochState, exactly mirroring
/// what consume_randomness does at lines 263-269 when Carnage triggers.
///
/// # Arguments
/// - `target`: 0 = CRIME, 1 = FRAUD (which token to buy)
/// - `action`: 0 = None (BuyOnly), 1 = Burn, 2 = Sell
pub fn handler(ctx: Context<ForceCarnage>, target: u8, action: u8) -> Result<()> {
    let epoch_state = &mut ctx.accounts.epoch_state;
    let clock = Clock::get()?;

    // Validate target (0=CRIME, 1=FRAUD)
    require!(target <= 1, EpochError::InvalidCarnageTargetPool);

    // Validate action (0=None, 1=Burn, 2=Sell)
    require!(action <= 2, EpochError::InvalidCarnageTargetPool);

    // Set pending state - identical to consume_randomness
    epoch_state.carnage_pending = true;
    epoch_state.carnage_generation = epoch_state.current_epoch;
    epoch_state.carnage_action = action;
    epoch_state.carnage_target = target;
    epoch_state.carnage_deadline_slot = clock
        .slot
        .checked_add(CARNAGE_DEADLINE_SLOTS)
        .ok_or(EpochError::Overflow)?;
    // VRF-01: Set carnage_lock_slot identically to consume_randomness.
    // During the lock window (0..CARNAGE_LOCK_SLOTS), only atomic Carnage
    // can execute. After lock expires, fallback path becomes callable.
    epoch_state.carnage_lock_slot = clock
        .slot
        .checked_add(CARNAGE_LOCK_SLOTS)
        .ok_or(EpochError::Overflow)?;

    msg!(
        "DEVNET: Forced Carnage! action={}, target={}, deadline={}",
        action,
        target,
        epoch_state.carnage_deadline_slot
    );

    Ok(())
}
