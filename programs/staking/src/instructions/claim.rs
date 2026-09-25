//! Claim pending SOL rewards without unstaking.
//!
//! Flow:
//! 1. Validate ownership
//! 2. Call update_rewards to finalize pending rewards
//! 3. Verify rewards_earned > 0
//! 4. Verify escrow has sufficient balance
//! 5. Transfer SOL from escrow to user
//! 6. Update state
//! 7. Emit Claimed event
//!
//! Security:
//! - Ownership check (SEC-05)
//! - update_rewards before claim (checkpoint pattern)
//! - CEI pattern (SEC-07)
//! - Escrow solvency check (ERR-03)
//!
//! Source: Docs/New_Yield_System_Spec.md Section 7.4, INST-04

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{rent::Rent, sysvar::Sysvar};

use crate::constants::{ESCROW_VAULT_SEED, STAKE_POOL_SEED, USER_STAKE_SEED};
use crate::errors::StakingError;
use crate::events::{Claimed, EscrowInsufficientAttempt};
use crate::helpers::update_rewards;
use crate::state::{StakePool, UserStake};

/// Accounts for claim instruction.
#[derive(Accounts)]
pub struct Claim<'info> {
    /// User claiming rewards (receives SOL).
    #[account(mut)]
    pub user: Signer<'info>,

    /// Global stake pool state.
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump = stake_pool.bump,
    )]
    pub stake_pool: Account<'info, StakePool>,

    /// User's stake account.
    #[account(
        mut,
        seeds = [USER_STAKE_SEED, user.key().as_ref()],
        bump = user_stake.bump,
        constraint = user_stake.owner == user.key() @ StakingError::Unauthorized,
    )]
    pub user_stake: Account<'info, UserStake>,

    /// Escrow vault PDA (source of SOL rewards).
    /// CHECK: PDA owned by this program, validated by seeds
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED],
        bump,
    )]
    pub escrow_vault: AccountInfo<'info>,

    /// System program.
    pub system_program: Program<'info, System>,
}

/// Claim pending SOL rewards without unstaking.
///
/// # Arguments
/// None - claims all pending rewards
///
/// # Errors
/// * `Unauthorized` - If signer doesn't own UserStake
/// * `NothingToClaim` - If rewards_earned is 0
/// * `InsufficientEscrowBalance` - If escrow can't cover rewards
///
/// # Events
/// * `Claimed` - Emitted with user, amount, staked_balance, total_claimed
/// * `EscrowInsufficientAttempt` - Emitted before InsufficientEscrowBalance error
pub fn handler(ctx: Context<Claim>) -> Result<()> {
    let pool = &mut ctx.accounts.stake_pool;
    let user = &mut ctx.accounts.user_stake;
    let clock = Clock::get()?;

    // === CHECKS ===

    // Verify user is owner (redundant with constraint, but explicit for clarity)
    require!(
        user.owner == ctx.accounts.user.key(),
        StakingError::Unauthorized
    );

    // Update rewards to finalize pending
    update_rewards(pool, user)?;

    // Check that there are rewards to claim
    require!(user.rewards_earned > 0, StakingError::NothingToClaim);

    // Capture rewards amount
    let rewards_to_claim = user.rewards_earned;

    // Verify escrow has sufficient balance after reserving rent-exempt minimum.
    // Without this guard, a claim could drain the escrow PDA below the rent-exempt
    // threshold, causing the runtime to garbage-collect the account.
    let escrow_balance = ctx.accounts.escrow_vault.lamports();
    let rent = Rent::get()?;
    let rent_exempt_min = rent.minimum_balance(0);

    // Available = escrow balance minus rent-exempt reservation
    let available = escrow_balance.checked_sub(rent_exempt_min).unwrap_or(0);

    if available < rewards_to_claim {
        emit!(EscrowInsufficientAttempt {
            user: ctx.accounts.user.key(),
            requested: rewards_to_claim,
            available,
            slot: clock.slot,
        });
        return Err(StakingError::InsufficientEscrowBalance.into());
    }

    // === EFFECTS (state updates before transfer) ===

    // Clear user's earned rewards
    user.rewards_earned = 0;

    // Start cooldown timer -- unstake blocked for COOLDOWN_SECONDS
    user.last_claim_ts = clock.unix_timestamp;

    // Update user's total claimed
    user.total_claimed = user
        .total_claimed
        .checked_add(rewards_to_claim)
        .ok_or(StakingError::Overflow)?;

    // Update pool's total claimed
    pool.total_claimed = pool
        .total_claimed
        .checked_add(rewards_to_claim)
        .ok_or(StakingError::Overflow)?;

    // Capture staked_balance for event before transfers
    let staked_balance = user.staked_balance;
    let total_claimed = user.total_claimed;

    // === INTERACTIONS (external transfer) ===

    // Transfer SOL from escrow to user
    **ctx.accounts.escrow_vault.try_borrow_mut_lamports()? = ctx
        .accounts
        .escrow_vault
        .lamports()
        .checked_sub(rewards_to_claim)
        .ok_or(StakingError::Underflow)?;

    **ctx.accounts.user.try_borrow_mut_lamports()? = ctx
        .accounts
        .user
        .lamports()
        .checked_add(rewards_to_claim)
        .ok_or(StakingError::Overflow)?;

    // Emit event
    emit!(Claimed {
        user: ctx.accounts.user.key(),
        amount: rewards_to_claim,
        staked_balance,
        total_claimed,
        slot: clock.slot,
    });

    msg!(
        "Claimed: user={}, amount={}, staked={}, total_claimed={}",
        ctx.accounts.user.key(),
        rewards_to_claim,
        staked_balance,
        total_claimed
    );

    Ok(())
}
