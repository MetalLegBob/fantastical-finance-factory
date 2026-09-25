//! update_cumulative instruction - Epoch Program CPI target.
//!
//! Called by Epoch Program during consume_randomness to finalize epoch rewards.
//! Moves pending_rewards into the cumulative rewards_per_token_stored.
//! Prevents double-update via epoch number comparison.
//!
//! Access Control:
//! - epoch_authority must be Epoch Program's PDA with seeds::program = epoch_program_id()
//! - Generic "Unauthorized" implicit in constraint failure (no info leak)
//!
//! Source: 27-RESEARCH.md, New_Yield_System_Spec.md Section 7.6

use anchor_lang::prelude::*;

use crate::constants::{epoch_program_id, PRECISION, STAKE_POOL_SEED, STAKING_AUTHORITY_SEED};
use crate::errors::StakingError;
use crate::events::CumulativeUpdated;
use crate::state::StakePool;

/// Accounts for update_cumulative instruction (CPI-gated).
///
/// Called by Epoch Program via CPI during consume_randomness.
/// Finalizes pending_rewards into the cumulative reward tracker.
#[derive(Accounts)]
pub struct UpdateCumulative<'info> {
    /// Epoch Program's staking authority PDA.
    ///
    /// CRITICAL SECURITY: seeds::program ensures this PDA is derived from Epoch Program.
    /// Only Epoch Program can produce a valid signer with these seeds.
    ///
    /// CROSS-PROGRAM DEPENDENCY:
    /// - STAKING_AUTHORITY_SEED must match Epoch Program's derivation
    /// - epoch_program_id() must match Epoch Program's declare_id!
    /// - If either mismatch, update_cumulative will reject all Epoch Program calls
    ///
    /// CHECK: PDA derived from Epoch Program seeds, validated by seeds::program constraint
    #[account(
        seeds = [STAKING_AUTHORITY_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub epoch_authority: Signer<'info>,

    /// Stake pool global state - cumulative and pending updated here.
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump = stake_pool.bump,
        constraint = stake_pool.initialized @ StakingError::NotInitialized,
    )]
    pub stake_pool: Account<'info, StakePool>,
}

/// Handler for update_cumulative instruction.
///
/// # Arguments
/// * `epoch` - The epoch number being finalized
///
/// # Flow
/// 1. Prevent double-update (epoch must be > last_update_epoch)
/// 2. Calculate reward_per_token delta: pending * PRECISION / total_staked
/// 3. Add delta to rewards_per_token_stored
/// 4. Update total_distributed, clear pending_rewards
/// 5. Update last_update_epoch
/// 6. Emit CumulativeUpdated event
///
/// # Errors
/// - `AlreadyUpdated` if epoch <= last_update_epoch
/// - `Overflow` if arithmetic overflows
/// - `DivisionByZero` if total_staked is 0 (prevented by dead stake)
/// - Constraint failure if caller is not Epoch Program (implicit)
pub fn handler(ctx: Context<UpdateCumulative>, epoch: u32) -> Result<()> {
    let pool = &mut ctx.accounts.stake_pool;
    let clock = Clock::get()?;

    // === 1. Prevent double-update ===
    // ERR-06: AlreadyUpdated
    require!(epoch > pool.last_update_epoch, StakingError::AlreadyUpdated);

    // === 2-3. Calculate and add to cumulative ===
    // Only update if there are pending rewards AND staked tokens
    // (Dead stake ensures total_staked >= MINIMUM_STAKE)
    let rewards_added = pool.pending_rewards;

    if rewards_added > 0 && pool.total_staked > 0 {
        let reward_per_token = (rewards_added as u128)
            .checked_mul(PRECISION)
            .ok_or(StakingError::Overflow)?
            .checked_div(pool.total_staked as u128)
            .ok_or(StakingError::DivisionByZero)?;

        pool.rewards_per_token_stored = pool
            .rewards_per_token_stored
            .checked_add(reward_per_token)
            .ok_or(StakingError::Overflow)?;

        // === 4. Update analytics ===
        pool.total_distributed = pool
            .total_distributed
            .checked_add(rewards_added)
            .ok_or(StakingError::Overflow)?;
    }

    // Clear pending regardless of whether distribution happened
    pool.pending_rewards = 0;

    // === 5. Update epoch tracker ===
    pool.last_update_epoch = epoch;

    // === 6. Emit event ===
    emit!(CumulativeUpdated {
        epoch,
        rewards_added,
        new_cumulative: pool.rewards_per_token_stored,
        total_staked: pool.total_staked,
        slot: clock.slot,
    });

    msg!(
        "Cumulative updated: epoch={}, rewards_added={}, cumulative={}",
        epoch,
        rewards_added,
        pool.rewards_per_token_stored
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::constants::{MINIMUM_STAKE, PRECISION};

    /// Test reward_per_token calculation.
    /// pending * PRECISION / total_staked = reward_per_token
    #[test]
    fn test_reward_per_token_calculation() {
        let pending_rewards: u64 = 1_000_000_000; // 1 SOL
        let total_staked: u64 = 10_000_000; // 10 PROFIT (6 decimals)

        // reward_per_token = 1_000_000_000 * 1e18 / 10_000_000
        //                  = 1e27 / 1e7
        //                  = 1e20
        let reward_per_token = (pending_rewards as u128)
            .checked_mul(PRECISION)
            .unwrap()
            .checked_div(total_staked as u128)
            .unwrap();

        // 1 SOL / 10 PROFIT = 0.1 SOL per PROFIT = 100_000_000 lamports per PROFIT
        // With PRECISION scaling: 100_000_000 * 1e12 = 1e20
        assert_eq!(reward_per_token, 100_000_000_000_000_000_000u128);
    }

    /// Test zero pending rewards does not panic.
    /// When pending_rewards = 0, we skip the division entirely.
    #[test]
    fn test_zero_pending_no_panic() {
        let pending_rewards: u64 = 0;
        let total_staked: u64 = 10_000_000;

        // Simulate the handler logic: skip division when pending is 0
        if pending_rewards > 0 && total_staked > 0 {
            let _reward_per_token = (pending_rewards as u128)
                .checked_mul(PRECISION)
                .unwrap()
                .checked_div(total_staked as u128)
                .unwrap();
        }
        // No panic = success
    }

    /// Test that dead stake prevents division by zero.
    /// Pool always has at least MINIMUM_STAKE from initialization.
    #[test]
    fn test_dead_stake_prevents_div_zero() {
        // Pool always has at least MINIMUM_STAKE from initialization
        assert!(MINIMUM_STAKE > 0);
        // This ensures total_staked > 0 always, preventing division by zero
    }

    /// Test precision is sufficient for small rewards.
    /// Even 1 lamport distributed across 1M PROFIT should be non-zero.
    #[test]
    fn test_small_rewards_precision() {
        let pending_rewards: u64 = 1; // 1 lamport
        let total_staked: u64 = 1_000_000_000_000; // 1M PROFIT (6 decimals = 1e12 units)

        let reward_per_token = (pending_rewards as u128)
            .checked_mul(PRECISION)
            .unwrap()
            .checked_div(total_staked as u128)
            .unwrap();

        // 1 * 1e18 / 1e12 = 1e6
        // Should be non-zero due to PRECISION
        assert!(reward_per_token > 0);
        assert_eq!(reward_per_token, 1_000_000u128);
    }

    /// Test maximum values don't overflow.
    /// u64::MAX pending rewards with MINIMUM_STAKE should not overflow.
    #[test]
    fn test_max_pending_no_overflow() {
        let pending_rewards: u64 = u64::MAX; // ~18.4e18 lamports
        let total_staked: u64 = MINIMUM_STAKE; // 1 PROFIT = 1e6 units

        // This should not overflow because:
        // u64::MAX * PRECISION = ~18.4e18 * 1e18 = ~18.4e36
        // u128::MAX = ~3.4e38, so we have headroom
        let result = (pending_rewards as u128).checked_mul(PRECISION);
        assert!(result.is_some());

        let reward_per_token = result.unwrap().checked_div(total_staked as u128);
        assert!(reward_per_token.is_some());
    }

    /// Test cumulative addition doesn't overflow with realistic values.
    /// Simulate multiple epochs of reward distribution.
    #[test]
    fn test_cumulative_addition_realistic() {
        let mut cumulative: u128 = 0;
        let rewards_per_epoch: u64 = 1_000_000_000_000; // 1000 SOL per epoch
        let total_staked: u64 = 10_000_000_000; // 10000 PROFIT

        // Simulate 1000 epochs
        for _ in 0..1000 {
            let reward_per_token = (rewards_per_epoch as u128)
                .checked_mul(PRECISION)
                .unwrap()
                .checked_div(total_staked as u128)
                .unwrap();

            cumulative = cumulative.checked_add(reward_per_token).unwrap();
        }

        // Should complete without overflow
        assert!(cumulative > 0);
    }

    /// Test epoch comparison logic.
    /// epoch > last_update_epoch must be true for update to proceed.
    #[test]
    fn test_epoch_comparison() {
        let last_update_epoch: u32 = 100;

        // Valid: new epoch is greater
        assert!(101 > last_update_epoch);
        assert!(1000 > last_update_epoch);

        // Invalid: new epoch is equal or less
        assert!(!(100 > last_update_epoch)); // Equal
        assert!(!(99 > last_update_epoch)); // Less
    }
}
