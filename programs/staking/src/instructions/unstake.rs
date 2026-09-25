//! Unstake PROFIT tokens with reward forfeiture.
//!
//! Flow:
//! 1. Validate ownership and balance
//! 2. Enforce cooldown gate (COOLDOWN_SECONDS after last claim)
//! 3. Handle partial unstake: if remaining < MINIMUM_STAKE, auto-full-unstake
//! 4. Call update_rewards BEFORE balance change
//! 5. Forfeit pending rewards to pool (pending_rewards += rewards_earned)
//! 6. Transfer PROFIT from stake_vault to user
//! 7. Update balances, reset last_claim_ts if fully unstaked
//! 8. Emit Unstaked event
//!
//! Security:
//! - Ownership check (SEC-05)
//! - Cooldown gate prevents mercenary capital
//! - update_rewards before balance change (flash loan protection)
//! - CEI pattern: forfeiture state written before token transfer (SEC-07)
//! - Partial unstake minimum enforcement (26-CONTEXT.md)
//!
//! Source: Docs/New_Yield_System_Spec.md Section 7.3, INST-03

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    COOLDOWN_SECONDS, MINIMUM_STAKE, PROFIT_DECIMALS, STAKE_POOL_SEED, STAKE_VAULT_SEED,
    USER_STAKE_SEED,
};
use crate::errors::StakingError;
use crate::events::Unstaked;
use crate::helpers::{transfer_checked_with_hook, update_rewards};
use crate::state::{StakePool, UserStake};

/// Accounts for unstake instruction.
#[derive(Accounts)]
pub struct Unstake<'info> {
    /// User unstaking tokens (receives PROFIT back).
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

    /// User's PROFIT token account (destination for unstaked tokens).
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = user,
        token::token_program = token_program,
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    /// Stake vault PDA (source for unstaked tokens).
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
}

/// Unstake PROFIT tokens with reward forfeiture.
///
/// # Arguments
/// * `amount` - Amount of PROFIT tokens to unstake
///
/// # Partial Unstake Behavior (per 26-CONTEXT.md)
/// If remaining balance after unstake would be < MINIMUM_STAKE:
/// - Automatically unstake full balance instead
/// - Prevents dust positions that can't be further unstaked
///
/// # Errors
/// * `ZeroAmount` - If amount is 0
/// * `Unauthorized` - If signer doesn't own the UserStake
/// * `InsufficientBalance` - If amount > staked_balance
/// * `CooldownActive` - If < COOLDOWN_SECONDS since last claim
///
/// # Events
/// * `Unstaked` - Emitted with user, amount, rewards_forfeited, new_balance
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, Unstake<'info>>,
    mut amount: u64,
) -> Result<()> {
    // === CHECKS ===
    require!(amount > 0, StakingError::ZeroAmount);

    let clock = Clock::get()?;

    // Ownership verified by constraint, but explicit check for clarity
    require!(
        ctx.accounts.user_stake.owner == ctx.accounts.user.key(),
        StakingError::Unauthorized
    );

    require!(
        ctx.accounts.user_stake.staked_balance >= amount,
        StakingError::InsufficientBalance
    );

    // Cooldown gate: must wait COOLDOWN_SECONDS after last claim
    // Skip check if user has never claimed (last_claim_ts == 0)
    if ctx.accounts.user_stake.last_claim_ts > 0 {
        let elapsed = clock
            .unix_timestamp
            .checked_sub(ctx.accounts.user_stake.last_claim_ts)
            .unwrap_or(0); // clock weirdness -> treat as cooldown active
        require!(elapsed >= COOLDOWN_SECONDS, StakingError::CooldownActive);
    }

    // Handle partial unstake: if remaining < MINIMUM_STAKE, auto-full-unstake
    // This prevents dust positions that can't be further unstaked
    let remaining_after = ctx
        .accounts
        .user_stake
        .staked_balance
        .saturating_sub(amount);
    if remaining_after > 0 && remaining_after < MINIMUM_STAKE {
        amount = ctx.accounts.user_stake.staked_balance; // Full unstake instead
    }

    // Capture pool bump before mutable borrow
    let pool_bump = ctx.accounts.stake_pool.bump;

    // Create scope for mutable borrows to complete before CPI
    let (rewards_forfeited, new_user_balance, new_total_staked) = {
        let pool = &mut ctx.accounts.stake_pool;
        let user = &mut ctx.accounts.user_stake;

        // Update rewards BEFORE balance change (checkpoint pattern)
        update_rewards(pool, user)?;

        // === EFFECTS (state updates before transfers) ===

        // Forfeit unclaimed rewards to pool (redistributed to remaining stakers)
        let rewards_forfeited = user.rewards_earned;
        if rewards_forfeited > 0 {
            pool.pending_rewards = pool
                .pending_rewards
                .checked_add(rewards_forfeited)
                .ok_or(StakingError::Overflow)?;
            user.rewards_earned = 0;
        }

        // Update balances
        user.staked_balance = user
            .staked_balance
            .checked_sub(amount)
            .ok_or(StakingError::Underflow)?;

        pool.total_staked = pool
            .total_staked
            .checked_sub(amount)
            .ok_or(StakingError::Underflow)?;

        // Reset cooldown on full exit (clean slate for re-staking)
        if user.staked_balance == 0 {
            user.last_claim_ts = 0;
        }

        (rewards_forfeited, user.staked_balance, pool.total_staked)
    };

    // === INTERACTIONS (external transfer) ===

    // Transfer PROFIT from stake_vault to user
    // Use manual CPI with remaining_accounts for Transfer Hook support.
    // Anchor's built-in transfer_checked does not forward remaining_accounts
    // to invoke_signed, which breaks Token-2022 transfer hooks.
    let bump_bytes = [pool_bump];
    let pool_seeds: &[&[u8]] = &[STAKE_POOL_SEED, &bump_bytes];
    let signer_seeds: &[&[&[u8]]] = &[pool_seeds];

    transfer_checked_with_hook(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.stake_vault.to_account_info(),
        &ctx.accounts.profit_mint.to_account_info(),
        &ctx.accounts.user_token_account.to_account_info(),
        &ctx.accounts.stake_pool.to_account_info(),
        ctx.remaining_accounts,
        amount,
        PROFIT_DECIMALS,
        signer_seeds,
    )?;

    // Emit event
    emit!(Unstaked {
        user: ctx.accounts.user.key(),
        amount,
        rewards_forfeited,
        new_balance: new_user_balance,
        total_staked: new_total_staked,
        slot: clock.slot,
    });

    msg!(
        "Unstaked: user={}, amount={}, forfeited={}, new_balance={}",
        ctx.accounts.user.key(),
        amount,
        rewards_forfeited,
        new_user_balance
    );

    Ok(())
}
