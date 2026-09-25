//! UserStake account - Per-user stake position.
//!
//! This account tracks an individual user's stake position:
//! - Staked PROFIT balance
//! - Reward checkpoint (rewards_per_token_paid)
//! - Accumulated unclaimed rewards
//! - Analytics for total claimed and first stake time
//!
//! PDA Derivation: seeds = ["user_stake", user_pubkey]
//!
//! Source: Docs/New_Yield_System_Spec.md Section 5.4

use anchor_lang::prelude::*;

/// UserStake per-user account.
///
/// Seeds: ["user_stake", user_pubkey]
/// Size: 105 bytes (8 discriminator + 97 data)
#[account]
pub struct UserStake {
    /// Owner of this stake account.
    /// Validated on unstake/claim to prevent unauthorized access.
    pub owner: Pubkey,

    /// Amount of PROFIT staked.
    /// Updated on stake/unstake operations.
    pub staked_balance: u64,

    /// User's checkpoint of rewards_per_token at last update.
    /// Used to calculate pending rewards since last interaction.
    pub rewards_per_token_paid: u128,

    /// Accumulated rewards not yet claimed.
    /// Updated by update_rewards helper before any balance change.
    pub rewards_earned: u64,

    /// Total SOL claimed lifetime (analytics).
    /// Incremented on each claim.
    pub total_claimed: u64,

    /// Slot when user first staked.
    /// Set once on first stake, never updated.
    pub first_stake_slot: u64,

    /// Slot when user last interacted (stake/unstake/claim).
    /// Updated by update_rewards helper.
    pub last_update_slot: u64,

    /// Unix timestamp of user's last claim.
    /// Used for cooldown gate: unstake blocked until COOLDOWN_SECONDS after last claim.
    /// 0 = never claimed (no cooldown applies).
    pub last_claim_ts: i64,

    /// PDA bump seed.
    /// Stored for efficient re-derivation.
    pub bump: u8,
}

impl UserStake {
    /// Account size including discriminator.
    ///
    /// Layout:
    /// - 8 bytes: Anchor discriminator
    /// - 32 bytes: owner (Pubkey)
    /// - 8 bytes: staked_balance (u64)
    /// - 16 bytes: rewards_per_token_paid (u128)
    /// - 8 bytes: rewards_earned (u64)
    /// - 8 bytes: total_claimed (u64)
    /// - 8 bytes: first_stake_slot (u64)
    /// - 8 bytes: last_update_slot (u64)
    /// - 8 bytes: last_claim_ts (i64)
    /// - 1 byte: bump (u8)
    ///
    /// Total: 8 + 32 + 8 + 16 + 8 + 8 + 8 + 8 + 8 + 1 = 105 bytes
    pub const LEN: usize = 8 + 32 + 8 + 16 + 8 + 8 + 8 + 8 + 8 + 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_stake_len() {
        // Verify LEN constant matches expected size
        assert_eq!(UserStake::LEN, 105);
    }
}
