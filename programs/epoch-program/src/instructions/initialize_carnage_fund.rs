//! initialize_carnage_fund instruction.
//!
//! Creates Carnage Fund state and vault accounts.
//! Called once at deployment, before protocol goes live.
//!
//! Source: Carnage_Fund_Spec.md Section 13.1

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    CARNAGE_CRIME_VAULT_SEED, CARNAGE_FRAUD_VAULT_SEED, CARNAGE_FUND_SEED, CARNAGE_SOL_VAULT_SEED,
};
use crate::errors::EpochError;
use crate::events::CarnageFundInitialized;
use crate::state::CarnageFundState;

/// Handler for initialize_carnage_fund instruction.
///
/// Initializes CarnageFundState with vault addresses and zeroed statistics.
/// Token vaults are created via Anchor's init constraint.
///
/// # Arguments
/// None - all values are hardcoded for initialization.
///
/// # Errors
/// - `CarnageAlreadyInitialized` if called more than once (defense-in-depth)
pub fn handler(ctx: Context<InitializeCarnageFund>) -> Result<()> {
    let carnage_state = &mut ctx.accounts.carnage_state;
    let clock = Clock::get()?;

    // Verify not already initialized (defense-in-depth, Anchor's init should prevent this)
    require!(
        !carnage_state.initialized,
        EpochError::CarnageAlreadyInitialized
    );

    // Initialize state fields
    carnage_state.sol_vault = ctx.accounts.sol_vault.key();
    carnage_state.crime_vault = ctx.accounts.crime_vault.key();
    carnage_state.fraud_vault = ctx.accounts.fraud_vault.key();
    carnage_state.held_token = 0; // HeldToken::None
    carnage_state.held_amount = 0;
    carnage_state.last_trigger_epoch = 0;
    carnage_state.total_sol_spent = 0;
    carnage_state.total_crime_burned = 0;
    carnage_state.total_fraud_burned = 0;
    carnage_state.total_triggers = 0;
    carnage_state.initialized = true;
    carnage_state.bump = ctx.bumps.carnage_state;

    msg!(
        "Carnage Fund initialized: sol_vault={}, crime_vault={}, fraud_vault={}",
        carnage_state.sol_vault,
        carnage_state.crime_vault,
        carnage_state.fraud_vault
    );

    emit!(CarnageFundInitialized {
        sol_vault: carnage_state.sol_vault,
        crime_vault: carnage_state.crime_vault,
        fraud_vault: carnage_state.fraud_vault,
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

/// Accounts for initialize_carnage_fund instruction.
#[derive(Accounts)]
pub struct InitializeCarnageFund<'info> {
    /// Deployer (one-time, pays for account creation)
    #[account(mut)]
    pub authority: Signer<'info>,

    /// Carnage Fund state account (PDA)
    #[account(
        init,
        payer = authority,
        space = CarnageFundState::LEN,
        seeds = [CARNAGE_FUND_SEED],
        bump,
    )]
    pub carnage_state: Account<'info, CarnageFundState>,

    /// SOL vault PDA (SystemAccount holding native SOL).
    /// Note: This is just a PDA address that will receive lamports.
    /// We don't create an account here - SOL is stored as lamports in the PDA.
    /// CHECK: PDA derived from known seeds, will hold native SOL
    #[account(
        seeds = [CARNAGE_SOL_VAULT_SEED],
        bump,
    )]
    pub sol_vault: SystemAccount<'info>,

    /// CRIME token vault PDA (Token-2022 account)
    #[account(
        init,
        payer = authority,
        seeds = [CARNAGE_CRIME_VAULT_SEED],
        bump,
        token::mint = crime_mint,
        token::authority = carnage_state,
        token::token_program = token_program,
    )]
    pub crime_vault: InterfaceAccount<'info, TokenAccount>,

    /// FRAUD token vault PDA (Token-2022 account)
    #[account(
        init,
        payer = authority,
        seeds = [CARNAGE_FRAUD_VAULT_SEED],
        bump,
        token::mint = fraud_mint,
        token::authority = carnage_state,
        token::token_program = token_program,
    )]
    pub fraud_vault: InterfaceAccount<'info, TokenAccount>,

    /// CRIME token mint (Token-2022)
    pub crime_mint: InterfaceAccount<'info, Mint>,

    /// FRAUD token mint (Token-2022)
    pub fraud_mint: InterfaceAccount<'info, Mint>,

    /// Token-2022 program (for CRIME and FRAUD vaults)
    pub token_program: Interface<'info, TokenInterface>,

    /// The Epoch program — used to look up its ProgramData address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::EpochProgram>,

    /// ProgramData account — upgrade_authority must match authority.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(authority.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    /// System program
    pub system_program: Program<'info, System>,
}
