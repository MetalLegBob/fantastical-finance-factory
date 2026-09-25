//! Stake PROFIT tokens to begin earning yield.
//!
//! Flow:
//! 1. Initialize UserStake if new user
//! 2. Call update_rewards BEFORE balance change (checkpoint pattern)
//! 3. Transfer PROFIT from user to stake_vault
//! 4. Update staked_balance and total_staked
//! 5. Emit Staked event
//!
//! Security:
//! - update_rewards called before balance change (flash loan protection)
//! - CEI pattern: state updates before external transfer (reentrancy protection)
//! - ZeroAmount check prevents dust operations
//!
//! Source: Docs/New_Yield_System_Spec.md Section 7.2, INST-02

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{PROFIT_DECIMALS, STAKE_POOL_SEED, STAKE_VAULT_SEED, USER_STAKE_SEED};
use crate::errors::StakingError;
use crate::events::Staked;
use crate::helpers::{transfer_checked_with_hook, update_rewards};
use crate::state::{StakePool, UserStake};

/// Accounts for stake instruction.
#[derive(Accounts)]
pub struct Stake<'info> {
    /// User staking tokens.
    #[account(mut)]
    pub user: Signer<'info>,

    /// Global stake pool state.
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump = stake_pool.bump,
    )]
    pub stake_pool: Account<'info, StakePool>,

    /// User's stake account (created if doesn't exist).
    /// Seeds: ["user_stake", user_pubkey]
    #[account(
        init_if_needed,
        payer = user,
        space = UserStake::LEN,
        seeds = [USER_STAKE_SEED, user.key().as_ref()],
        bump,
    )]
    pub user_stake: Account<'info, UserStake>,

    /// User's PROFIT token account (source).
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = user,
        token::token_program = token_program,
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    /// Stake vault PDA (destination).
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = stake_pool,
        token::token_program = token_program,
        seeds = [STAKE_VAULT_SEED],
        bump,
    )]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,

    /// PROFIT token mint.
    pub profit_mint: InterfaceAccount<'info, Mint>,

    /// Token-2022 program.
    pub token_program: Interface<'info, TokenInterface>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}

/// Stake PROFIT tokens.
///
/// # Arguments
/// * `amount` - Amount of PROFIT tokens to stake (in base units, 6 decimals)
///
/// # Errors
/// * `ZeroAmount` - If amount is 0
/// * `Overflow` - If arithmetic overflows
///
/// # Events
/// * `Staked` - Emitted with user, amount, new_balance, total_staked
pub fn handler<'info>(ctx: Context<'_, '_, 'info, 'info, Stake<'info>>, amount: u64) -> Result<()> {
    // === CHECKS ===
    require!(amount > 0, StakingError::ZeroAmount);

    let pool = &mut ctx.accounts.stake_pool;
    let user = &mut ctx.accounts.user_stake;
    let clock = Clock::get()?;

    // Initialize user account if new (first stake)
    // Check if account was just created by init_if_needed
    let is_new_user = user.owner == Pubkey::default();

    if is_new_user {
        user.owner = ctx.accounts.user.key();
        user.rewards_per_token_paid = pool.rewards_per_token_stored;
        user.first_stake_slot = clock.slot;
        user.bump = ctx.bumps.user_stake;
        user.last_claim_ts = 0;
    }

    // Update rewards BEFORE balance change (checkpoint pattern)
    // This calculates rewards on the OLD balance
    update_rewards(pool, user)?;

    // === EFFECTS (state updates before transfer) ===
    user.staked_balance = user
        .staked_balance
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;

    pool.total_staked = pool
        .total_staked
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;

    // === INTERACTIONS (external transfer) ===
    // Use manual CPI with remaining_accounts for Transfer Hook support.
    // Anchor's built-in transfer_checked does not forward remaining_accounts
    // to invoke_signed, which breaks Token-2022 transfer hooks.
    transfer_checked_with_hook(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.user_token_account.to_account_info(),
        &ctx.accounts.profit_mint.to_account_info(),
        &ctx.accounts.stake_vault.to_account_info(),
        &ctx.accounts.user.to_account_info(),
        ctx.remaining_accounts,
        amount,
        PROFIT_DECIMALS,
        &[], // user is a signer, not a PDA
    )?;

    // Emit event
    emit!(Staked {
        user: ctx.accounts.user.key(),
        amount,
        new_balance: user.staked_balance,
        total_staked: pool.total_staked,
        slot: clock.slot,
    });

    msg!(
        "Staked: user={}, amount={}, new_balance={}, total={}",
        ctx.accounts.user.key(),
        amount,
        user.staked_balance,
        pool.total_staked
    );

    Ok(())
}
