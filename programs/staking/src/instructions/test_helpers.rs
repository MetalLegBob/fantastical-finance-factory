//! Test-only helper instructions.
//!
//! These instructions bypass CPI gating to allow unit tests to deposit
//! rewards and distribute them without deploying Tax/Epoch programs.
//! Only compiled when the `test` feature is enabled.
//!
//! SAFETY: These MUST NOT exist in production builds. The `#[cfg(feature = "test")]`
//! on the module ensures they are stripped from non-test builds entirely.

use anchor_lang::prelude::*;
use anchor_lang::system_program;

use crate::constants::{ESCROW_VAULT_SEED, PRECISION, STAKE_POOL_SEED};
use crate::errors::StakingError;
use crate::state::StakePool;

/// Deposit SOL rewards and distribute them to stakers in one step.
///
/// Combines what Tax Program (deposit_rewards) and Epoch Program
/// (update_cumulative) do in production, but without CPI gating.
#[derive(Accounts)]
pub struct TestDepositAndDistribute<'info> {
    /// Payer who provides the SOL.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Global stake pool state.
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump = stake_pool.bump,
        constraint = stake_pool.initialized @ StakingError::NotInitialized,
    )]
    pub stake_pool: Account<'info, StakePool>,

    /// Escrow vault PDA that holds SOL rewards.
    /// CHECK: PDA derived from known seeds, receives SOL transfer
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED],
        bump,
    )]
    pub escrow_vault: AccountInfo<'info>,

    /// System program for SOL transfer.
    pub system_program: Program<'info, System>,
}

/// Test-only: deposit SOL and distribute to stakers in one step.
///
/// 1. Transfers `amount` lamports from payer to escrow_vault
/// 2. Adds `amount` to pool.pending_rewards
/// 3. Distributes pending_rewards into rewards_per_token_stored
pub fn handler(ctx: Context<TestDepositAndDistribute>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);

    // Transfer SOL to escrow vault
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.escrow_vault.to_account_info(),
            },
        ),
        amount,
    )?;

    let pool = &mut ctx.accounts.stake_pool;

    // Add to pending rewards
    pool.pending_rewards = pool
        .pending_rewards
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;

    // Distribute: pending_rewards -> rewards_per_token_stored
    if pool.pending_rewards > 0 && pool.total_staked > 0 {
        let reward_per_token = (pool.pending_rewards as u128)
            .checked_mul(PRECISION)
            .ok_or(StakingError::Overflow)?
            .checked_div(pool.total_staked as u128)
            .ok_or(StakingError::DivisionByZero)?;

        pool.rewards_per_token_stored = pool
            .rewards_per_token_stored
            .checked_add(reward_per_token)
            .ok_or(StakingError::Overflow)?;

        pool.total_distributed = pool
            .total_distributed
            .checked_add(pool.pending_rewards)
            .ok_or(StakingError::Overflow)?;

        pool.pending_rewards = 0;
    }

    msg!(
        "Test: deposited {} lamports and distributed to stakers",
        amount
    );

    Ok(())
}
