//! Staking program instructions.
//!
//! This module exports all instruction handlers and account contexts:
//! - initialize_stake_pool: Create global pool and vaults
//! - stake: Deposit PROFIT tokens to begin earning yield
//! - unstake: Withdraw PROFIT tokens and auto-claim pending rewards
//! - claim: Collect pending SOL rewards without unstaking
//! - deposit_rewards: CPI target for Tax Program yield deposits
//! - update_cumulative: CPI target for Epoch Program cumulative finalization

pub mod claim;
pub mod deposit_rewards;
pub mod initialize_stake_pool;
pub mod stake;
pub mod unstake;
pub mod update_cumulative;

pub use claim::*;
pub use deposit_rewards::*;
pub use initialize_stake_pool::*;
pub use stake::*;
pub use unstake::*;
pub use update_cumulative::*;

#[cfg(feature = "test")]
pub mod test_helpers;
#[cfg(feature = "test")]
pub use test_helpers::*;
