//! Initialize the staking pool with dead stake to prevent first-depositor attack.
//!
//! Creates:
//! - StakePool PDA (global state)
//! - EscrowVault PDA (native SOL holder for rewards)
//! - StakeVault PDA (Token-2022 account for staked PROFIT)
//!
//! Security:
//! - Transfers MINIMUM_STAKE (1 PROFIT) from authority to stake vault
//! - This "dead stake" prevents first-depositor inflation attack
//! - Pool starts with total_staked = MINIMUM_STAKE
//!
//! Source: Docs/New_Yield_System_Spec.md Section 7.1, SEC-01

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    canonical_profit_mint, ESCROW_VAULT_SEED, MINIMUM_STAKE, PROFIT_DECIMALS, STAKE_POOL_SEED,
    STAKE_VAULT_SEED,
};
use crate::errors::StakingError;
use crate::events::StakePoolInitialized;
use crate::helpers::transfer_checked_with_hook;
use crate::state::StakePool;

/// Accounts for initialize_stake_pool instruction.
#[derive(Accounts)]
pub struct InitializeStakePool<'info> {
    /// Authority who pays for account creation and provides dead stake.
    /// Must own `authority_token_account` with at least MINIMUM_STAKE PROFIT.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// Global stake pool state PDA.
    /// Seeds: ["stake_pool"]
    #[account(
        init,
        payer = authority,
        space = StakePool::LEN,
        seeds = [STAKE_POOL_SEED],
        bump,
    )]
    pub stake_pool: Account<'info, StakePool>,

    /// Native SOL escrow vault PDA (holds undistributed yield).
    /// Seeds: ["escrow_vault"]
    /// This is a system account, not a token account.
    ///
    /// CHECK: PDA owned by system program, will receive SOL transfers
    #[account(
        init,
        payer = authority,
        space = 0,
        seeds = [ESCROW_VAULT_SEED],
        bump,
    )]
    pub escrow_vault: AccountInfo<'info>,

    /// Token-2022 stake vault PDA (holds staked PROFIT tokens).
    /// Seeds: ["stake_vault"]
    /// Authority: stake_pool PDA (so pool can transfer out on unstake)
    #[account(
        init,
        payer = authority,
        token::mint = profit_mint,
        token::authority = stake_pool,
        token::token_program = token_program,
        seeds = [STAKE_VAULT_SEED],
        bump,
    )]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,

    /// Authority's PROFIT token account (source of dead stake).
    /// Must have at least MINIMUM_STAKE tokens.
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = authority,
        token::token_program = token_program,
    )]
    pub authority_token_account: InterfaceAccount<'info, TokenAccount>,

    /// Canonical cluster-specific PROFIT token mint (Token-2022, 6 decimals).
    #[account(
        address = canonical_profit_mint() @ StakingError::InvalidProfitMint,
        constraint = profit_mint.to_account_info().owner == &anchor_spl::token_2022::ID
            @ StakingError::InvalidTokenProgram,
        constraint = profit_mint.decimals == PROFIT_DECIMALS
            @ StakingError::InvalidProfitDecimals,
    )]
    pub profit_mint: InterfaceAccount<'info, Mint>,

    /// Token-2022 program for PROFIT transfers.
    #[account(
        address = anchor_spl::token_2022::ID @ StakingError::InvalidTokenProgram,
    )]
    pub token_program: Interface<'info, TokenInterface>,

    /// The Staking program — used to look up its ProgramData address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::Staking>,

    /// ProgramData account — upgrade_authority must match authority.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(authority.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}

/// Initialize the stake pool with dead stake.
///
/// # Arguments
/// None - uses constants for configuration
///
/// # Security
/// - Transfers MINIMUM_STAKE PROFIT as dead stake (SEC-01)
/// - This stake is never claimable, preventing first-depositor attack
/// - Pool starts with total_staked = MINIMUM_STAKE, never goes to 0
///
/// # Events
/// - Emits StakePoolInitialized with vault addresses (EVNT-01)
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, InitializeStakePool<'info>>,
) -> Result<()> {
    let pool = &mut ctx.accounts.stake_pool;
    let clock = Clock::get()?;

    // Initialize pool state
    pool.total_staked = 0; // Will be set to MINIMUM_STAKE after transfer
    pool.rewards_per_token_stored = 0;
    pool.pending_rewards = 0;
    pool.last_update_epoch = 0;
    pool.total_distributed = 0;
    pool.total_claimed = 0;
    pool.initialized = true;
    pool.bump = ctx.bumps.stake_pool;

    // Transfer MINIMUM_STAKE (1 PROFIT) as dead stake
    // This prevents first-depositor attack by ensuring pool always has liquidity
    // Use manual CPI with remaining_accounts for Transfer Hook support.
    // Anchor's built-in transfer_checked does not forward remaining_accounts
    // to invoke_signed, which breaks Token-2022 transfer hooks.
    transfer_checked_with_hook(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.authority_token_account.to_account_info(),
        &ctx.accounts.profit_mint.to_account_info(),
        &ctx.accounts.stake_vault.to_account_info(),
        &ctx.accounts.authority.to_account_info(),
        ctx.remaining_accounts,
        MINIMUM_STAKE,
        PROFIT_DECIMALS,
        &[], // authority is a signer, not a PDA
    )?;

    // Update total_staked to reflect dead stake
    pool.total_staked = MINIMUM_STAKE;

    // Emit initialization event
    emit!(StakePoolInitialized {
        escrow_vault: ctx.accounts.escrow_vault.key(),
        stake_vault: ctx.accounts.stake_vault.key(),
        dead_stake_amount: MINIMUM_STAKE,
        timestamp: clock.unix_timestamp,
    });

    msg!(
        "Stake pool initialized: escrow={}, vault={}, dead_stake={}",
        ctx.accounts.escrow_vault.key(),
        ctx.accounts.stake_vault.key(),
        MINIMUM_STAKE
    );

    Ok(())
}
