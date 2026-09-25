//! Initialize ArbConfig instruction.
//!
//! Creates the singleton ArbConfig PDA and roots its authority in the deployed
//! program's verified upgrade-authority signer.

use anchor_lang::prelude::*;

use crate::constants::ARB_CONFIG_SEED;
use crate::events::ArbConfigUpdated;
use crate::state::{
    validate_arb_config_bounds, validate_no_zero_pubkeys, validate_role_overlap, ArbConfig,
};

/// Initialize the global ArbConfig singleton.
///
/// The verified ProgramData upgrade-authority signer becomes the initial
/// ArbConfig authority. There is no authority argument, eliminating the most
/// dangerous manual-entry typo surface. Anchor's `init` constraint rejects a
/// second initialization before this handler can run.
///
/// Source: Centralized-Arb MVP Spec §3.1, §14.1 ruling 13, and §14.6
/// entries 1, 5, and 6.
pub fn handler(
    ctx: Context<InitializeArbConfig>,
    wallet_a: Pubkey,
    wallet_b: Pubkey,
    pause_slots: u64,
    epoch_length_slots: u64,
    pause_authority: Pubkey,
    pause_tripwire: Pubkey,
) -> Result<()> {
    validate_arb_config_bounds(pause_slots, epoch_length_slots)?;
    validate_no_zero_pubkeys(&[wallet_a, wallet_b, pause_authority, pause_tripwire])?;

    let authority = ctx.accounts.payer.key();
    validate_role_overlap(
        &authority,
        &wallet_a,
        &wallet_b,
        &pause_authority,
        &pause_tripwire,
    )?;

    let slot = Clock::get()?.slot;
    let arb_config = &mut ctx.accounts.arb_config;
    arb_config.authority = authority;
    arb_config.pending_authority = Pubkey::default();
    arb_config.wallet_a = wallet_a;
    arb_config.wallet_b = wallet_b;
    arb_config.pause_authority = pause_authority;
    arb_config.pause_tripwire = pause_tripwire;
    arb_config.pause_slots = pause_slots;
    arb_config.epoch_length_slots = epoch_length_slots;
    arb_config.bump = ctx.bumps.arb_config;
    arb_config.reserved = [0u8; 64];

    emit!(ArbConfigUpdated {
        authority: arb_config.authority,
        pending_authority: arb_config.pending_authority,
        wallet_a: arb_config.wallet_a,
        wallet_b: arb_config.wallet_b,
        pause_authority: arb_config.pause_authority,
        pause_tripwire: arb_config.pause_tripwire,
        pause_slots: arb_config.pause_slots,
        epoch_length_slots: arb_config.epoch_length_slots,
        slot,
    });

    Ok(())
}

/// Accounts for `initialize_arb_config`.
///
/// The Program/ProgramData constraint pair is the same deployed
/// upgrade-authority gate used by `initialize_epoch_state`.
#[derive(Accounts)]
pub struct InitializeArbConfig<'info> {
    /// Payer for account creation rent and verified initial authority.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Global ArbConfig PDA. Anchor `init` makes re-initialization impossible.
    #[account(
        init,
        payer = payer,
        space = ArbConfig::LEN,
        seeds = [ARB_CONFIG_SEED],
        bump,
    )]
    pub arb_config: Account<'info, ArbConfig>,

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
