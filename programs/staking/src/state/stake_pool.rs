//! StakePool account - Global singleton for the staking system.
//!
//! This account tracks the global state of the staking pool:
//! - Total PROFIT staked across all users
//! - Cumulative reward-per-token (monotonically increasing)
//! - Pending rewards awaiting epoch finalization
//! - Analytics for total distributed and claimed
//!
//! PDA Derivation: seeds = ["stake_pool"]
//!
//! Source: Docs/New_Yield_System_Spec.md Section 5.1

use anchor_lang::prelude::*;

/// StakePool global singleton.
///
/// Seeds: ["stake_pool"]
/// Size: 62 bytes (8 discriminator + 54 data)
#[account]
pub struct StakePool {
    /// Total PROFIT currently staked across all users.
    /// Updated on every stake/unstake operation.
    pub total_staked: u64,

    /// Cumulative rewards per staked token, scaled by PRECISION (1e18).
    /// This value only increases, never decreases.
    /// Used for fair pro-rata reward distribution.
    pub rewards_per_token_stored: u128,

    /// SOL rewards accumulated this epoch, not yet added to cumulative.
    /// Reset to 0 after update_cumulative is called.
    pub pending_rewards: u64,

    /// Last epoch when cumulative was updated.
    /// Prevents double-update within same epoch.
    pub last_update_epoch: u32,

    /// Total SOL distributed lifetime (analytics).
    /// Incremented when pending_rewards is added to cumulative.
    pub total_distributed: u64,

    /// Total SOL claimed lifetime (analytics).
    /// Incremented when users claim rewards.
    pub total_claimed: u64,

    /// Initialization flag.
    /// Set to true during initialize_stake_pool.
    pub initialized: bool,

    /// PDA bump seed.
    /// Stored for efficient re-derivation during transfers.
    pub bump: u8,
}

impl StakePool {
    /// Account size including discriminator.
    ///
    /// Layout:
    /// - 8 bytes: Anchor discriminator
    /// - 8 bytes: total_staked (u64)
    /// - 16 bytes: rewards_per_token_stored (u128)
    /// - 8 bytes: pending_rewards (u64)
    /// - 4 bytes: last_update_epoch (u32)
    /// - 8 bytes: total_distributed (u64)
    /// - 8 bytes: total_claimed (u64)
    /// - 1 byte: initialized (bool)
    /// - 1 byte: bump (u8)
    ///
    /// Total: 8 + 8 + 16 + 8 + 4 + 8 + 8 + 1 + 1 = 62 bytes
    pub const LEN: usize = 8 + 8 + 16 + 8 + 4 + 8 + 8 + 1 + 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stake_pool_len() {
        // Verify LEN constant matches expected size
        assert_eq!(StakePool::LEN, 62);
    }
}
