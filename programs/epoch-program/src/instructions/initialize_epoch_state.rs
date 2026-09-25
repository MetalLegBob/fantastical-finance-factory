//! Initialize Epoch State instruction.
//!
//! One-time initialization at protocol deployment.
//! Creates the global EpochState singleton with genesis configuration.
//!
//! Source: Epoch_State_Machine_Spec.md Section 8.1

use anchor_lang::prelude::*;

use crate::constants::{
    EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED, GENESIS_HIGH_TAX_BPS, GENESIS_LOW_TAX_BPS,
};
use crate::errors::EpochError;
use crate::events::EpochStateInitialized;
use crate::state::EpochState;

/// Initialize the global epoch state.
///
/// Called once at protocol deployment by the deployer.
/// Sets up genesis configuration with CRIME as the cheap side.
///
/// # Genesis Configuration
/// - cheap_side: CRIME (0)
/// - low_tax_bps: 300 (3%)
/// - high_tax_bps: 1400 (14%)
/// - Derived rates:
///   - crime_buy: 300 bps (cheap to buy)
///   - crime_sell: 1400 bps (expensive to sell)
///   - fraud_buy: 1400 bps (expensive to buy)
///   - fraud_sell: 300 bps (cheap to sell)
///
/// # Errors
/// - `AlreadyInitialized` if called more than once
pub fn handler(ctx: Context<InitializeEpochState>) -> Result<()> {
    let clock = Clock::get()?;
    let epoch_state = &mut ctx.accounts.epoch_state;

    // Prevent re-initialization
    require!(!epoch_state.initialized, EpochError::AlreadyInitialized);

    // =========================================================================
    // Timing: Capture genesis slot
    // =========================================================================
    epoch_state.genesis_slot = clock.slot;
    epoch_state.current_epoch = 0;
    epoch_state.epoch_start_slot = clock.slot;

    // =========================================================================
    // Tax Configuration: Genesis with CRIME cheap
    // Source: Epoch_State_Machine_Spec.md Section 5
    // =========================================================================
    epoch_state.cheap_side = 0; // CRIME
    epoch_state.low_tax_bps = GENESIS_LOW_TAX_BPS; // 300 bps (3%)
    epoch_state.high_tax_bps = GENESIS_HIGH_TAX_BPS; // 1400 bps (14%)

    // Derived rates (CRIME cheap = CRIME low buy, high sell)
    epoch_state.crime_buy_tax_bps = GENESIS_LOW_TAX_BPS; // 300 bps
    epoch_state.crime_sell_tax_bps = GENESIS_HIGH_TAX_BPS; // 1400 bps
    epoch_state.fraud_buy_tax_bps = GENESIS_HIGH_TAX_BPS; // 1400 bps
    epoch_state.fraud_sell_tax_bps = GENESIS_LOW_TAX_BPS; // 300 bps

    // =========================================================================
    // VRF State: No pending request at genesis
    // =========================================================================
    epoch_state.vrf_request_slot = 0;
    epoch_state.vrf_pending = false;
    epoch_state.taxes_confirmed = true; // Genesis taxes are confirmed
    epoch_state.pending_randomness_account = Pubkey::default();

    // =========================================================================
    // Carnage State: No pending Carnage at genesis
    // =========================================================================
    epoch_state.carnage_pending = false;
    epoch_state.carnage_target = 0; // CRIME (ignored)
    epoch_state.carnage_action = 0; // None
    epoch_state.carnage_deadline_slot = 0;
    epoch_state.carnage_lock_slot = 0;
    epoch_state.last_carnage_epoch = 0;

    // =========================================================================
    // Pause state and remaining reserved bytes are zeroed for a safe default.
    // =========================================================================
    epoch_state.pause_end_slot = 0;
    epoch_state.trading_paused = false;
    epoch_state.safety_version = EPOCH_SAFETY_VERSION;
    epoch_state.vrf_transition_start_slot = 0;
    epoch_state.pending_seed_slot = 0;
    epoch_state.vrf_attempts = 0;
    epoch_state.carnage_generation = 0;
    epoch_state.last_degraded_epoch = 0;
    epoch_state.last_vrf_fallback_reason = 0;
    epoch_state.reserved = [0u8; 28];

    // =========================================================================
    // Protocol: Mark as initialized
    // =========================================================================
    epoch_state.initialized = true;
    epoch_state.bump = ctx.bumps.epoch_state;

    // =========================================================================
    // Emit initialization event
    // =========================================================================
    emit!(EpochStateInitialized {
        genesis_slot: clock.slot,
        initial_cheap_side: 0, // CRIME
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

/// Accounts for initialize_epoch_state instruction.
#[derive(Accounts)]
pub struct InitializeEpochState<'info> {
    /// Payer for account creation rent.
    /// Typically the protocol deployer.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Global epoch state PDA.
    /// seeds = ["epoch_state"]
    #[account(
        init,
        payer = payer,
        space = EpochState::LEN,
        seeds = [EPOCH_STATE_SEED],
        bump,
    )]
    pub epoch_state: Account<'info, EpochState>,

    /// The Epoch program — used to look up its ProgramData address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::EpochProgram>,

    /// ProgramData account — upgrade_authority must match payer.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(payer.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}
