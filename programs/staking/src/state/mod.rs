//! State module for staking program accounts.
//!
//! This module defines the on-chain account structures for the staking system:
//! - `StakePool`: Global singleton tracking total staked, cumulative rewards, etc.
//! - `UserStake`: Per-user account tracking individual stake position and earnings

pub mod stake_pool;
pub mod user_stake;

pub use stake_pool::StakePool;
pub use user_stake::UserStake;
