//! deposit_rewards instruction - Tax Program CPI target.
//!
//! Called by Tax Program after SOL transfer to update pending_rewards state.
//! The SOL is already in escrow_vault from system_instruction::transfer.
//! This instruction only updates StakePool.pending_rewards counter.
//!
//! Access Control:
//! - tax_authority must be Tax Program's PDA with seeds::program = tax_program_id()
//! - Generic "Unauthorized" implicit in constraint failure (no info leak)
//!
//! Source: 27-RESEARCH.md, New_Yield_System_Spec.md Section 7.5

use anchor_lang::prelude::*;

use crate::constants::{tax_program_id, ESCROW_VAULT_SEED, STAKE_POOL_SEED, TAX_AUTHORITY_SEED};
use crate::errors::StakingError;
use crate::events::RewardsDeposited;
use crate::state::StakePool;

/// Accounts for deposit_rewards instruction (CPI-gated).
///
/// Called by Tax Program via CPI after transferring SOL to escrow.
/// Updates pending_rewards counter without moving any tokens.
#[derive(Accounts)]
pub struct DepositRewards<'info> {
    /// Tax Program's tax authority PDA.
    ///
    /// CRITICAL SECURITY: seeds::program ensures this PDA is derived from Tax Program.
    /// Only Tax Program can produce a valid signer with these seeds.
    ///
    /// CROSS-PROGRAM DEPENDENCY:
    /// - TAX_AUTHORITY_SEED must match Tax Program's derivation
    /// - tax_program_id() must match Tax Program's declare_id!
    /// - If either mismatch, deposit_rewards will reject all Tax Program calls
    ///
    /// CHECK: PDA derived from Tax Program seeds, validated by seeds::program constraint
    #[account(
        seeds = [TAX_AUTHORITY_SEED],
        bump,
        seeds::program = tax_program_id(),
    )]
    pub tax_authority: Signer<'info>,

    /// Stake pool global state - pending_rewards updated here.
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump = stake_pool.bump,
        constraint = stake_pool.initialized @ StakingError::NotInitialized,
    )]
    pub stake_pool: Account<'info, StakePool>,

    /// SOL escrow vault PDA - used for balance reconciliation.
    ///
    /// After updating pending_rewards, we verify escrow_vault.lamports() >= pending_rewards.
    /// This catches silent transfer failures or short-changed CPI amounts.
    ///
    /// Note: AccountInfo (not SystemAccount) because this PDA was created via `init` in
    /// initialize_stake_pool, making the Staking Program the owner (not system program).
    ///
    /// CHECK: PDA derived from known seeds, only read for balance verification
    #[account(
        seeds = [ESCROW_VAULT_SEED],
        bump,
    )]
    pub escrow_vault: AccountInfo<'info>,
}

/// Handler for deposit_rewards instruction.
///
/// # Arguments
/// * `amount` - Amount of SOL deposited (already in escrow, just updating counter)
///
/// # Flow
/// 1. Validate amount > 0
/// 2. Add amount to pending_rewards
/// 3. Emit RewardsDeposited event
///
/// # Errors
/// - `ZeroAmount` if amount is 0
/// - `Overflow` if pending_rewards would overflow
/// - Constraint failure if caller is not Tax Program (implicit from seeds::program)
pub fn handler(ctx: Context<DepositRewards>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);

    let pool = &mut ctx.accounts.stake_pool;
    let clock = Clock::get()?;

    // Add to pending (SOL already transferred by Tax Program)
    pool.pending_rewards = pool
        .pending_rewards
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;

    // Reconcile: verify escrow vault actually holds enough SOL.
    // Tax Program should have transferred SOL before this CPI call.
    // If the transfer failed silently or the amount was short-changed,
    // pending_rewards would be inflated beyond available balance.
    require!(
        ctx.accounts.escrow_vault.lamports() >= pool.pending_rewards,
        StakingError::InsufficientEscrowBalance
    );

    emit!(RewardsDeposited {
        amount,
        new_pending: pool.pending_rewards,
        escrow_vault: ctx.accounts.escrow_vault.key(),
        escrow_balance: ctx.accounts.escrow_vault.lamports(),
        slot: clock.slot,
    });

    msg!(
        "Rewards deposited: {} lamports, pending={}",
        amount,
        pool.pending_rewards
    );

    Ok(())
}
