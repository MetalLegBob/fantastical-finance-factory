//! Events for the staking program.
//!
//! This module defines all event structs emitted by staking instructions.
//! Events follow the EVNT-XX naming convention from the spec:
//! - EVNT-01: StakePoolInitialized
//! - EVNT-02: Staked
//! - EVNT-03: Unstaked
//! - EVNT-04: Claimed
//!
//! Additional events for CPI operations (RewardsDeposited, CumulativeUpdated)
//! are included for completeness.
//!
//! All events include slot for indexer timeline reconstruction and
//! relevant pubkeys for filtering. Uses Anchor emit!() macro.
//!
//! Source: Docs/New_Yield_System_Spec.md Section 10

use anchor_lang::prelude::*;

/// EVNT-01: Emitted when stake pool is initialized.
///
/// Includes vault addresses for indexers to track and timestamp
/// for initialization timeline.
#[event]
pub struct StakePoolInitialized {
    /// The SOL escrow vault PDA address.
    pub escrow_vault: Pubkey,

    /// The PROFIT stake vault PDA address.
    pub stake_vault: Pubkey,

    /// Amount of dead stake deposited (MINIMUM_STAKE).
    /// This is the protocol's initial stake to prevent first-depositor attack.
    pub dead_stake_amount: u64,

    /// Unix timestamp from Clock sysvar.
    pub timestamp: i64,
}

/// EVNT-02: Emitted when a user stakes PROFIT.
///
/// Contains all information needed for indexers to track stake activity
/// without additional RPC lookups.
#[event]
pub struct Staked {
    /// The user who staked.
    pub user: Pubkey,

    /// Amount of PROFIT staked in this transaction.
    pub amount: u64,

    /// User's new total staked balance after this stake.
    pub new_balance: u64,

    /// Pool's new total staked after this stake.
    pub total_staked: u64,

    /// Slot when stake occurred.
    pub slot: u64,
}

/// EVNT-03: Emitted when a user unstakes PROFIT.
///
/// Includes the unstaked amount and any rewards forfeited back to the pool.
/// Unstake forfeits unclaimed rewards to remaining stakers.
#[event]
pub struct Unstaked {
    /// The user who unstaked.
    pub user: Pubkey,

    /// Amount of PROFIT unstaked in this transaction.
    pub amount: u64,

    /// Amount of SOL rewards forfeited back to the staking pool.
    /// Redistributed to remaining stakers via pending_rewards.
    pub rewards_forfeited: u64,

    /// User's new staked balance after unstake (may be 0).
    pub new_balance: u64,

    /// Pool's new total staked after unstake.
    pub total_staked: u64,

    /// Slot when unstake occurred.
    pub slot: u64,
}

/// EVNT-04: Emitted when a user claims SOL rewards.
///
/// Separate from unstake - user can claim without changing stake position.
#[event]
pub struct Claimed {
    /// The user who claimed.
    pub user: Pubkey,

    /// Amount of SOL claimed.
    pub amount: u64,

    /// User's current staked balance (unchanged by claim).
    pub staked_balance: u64,

    /// User's lifetime total claimed after this claim.
    pub total_claimed: u64,

    /// Slot when claim occurred.
    pub slot: u64,
}

/// Emitted when claim fails due to insufficient escrow balance.
///
/// This should never happen in normal operation - it indicates a bug
/// in reward accounting or an external exploit. The event is emitted
/// before the error is returned so it gets logged even on failure,
/// enabling monitoring/alerting systems to detect anomalies.
#[event]
pub struct EscrowInsufficientAttempt {
    /// The user who attempted to claim.
    pub user: Pubkey,

    /// Amount of SOL the user tried to claim (lamports).
    pub requested: u64,

    /// Actual SOL balance available in the escrow vault (lamports).
    pub available: u64,

    /// Slot when the failed attempt occurred.
    pub slot: u64,
}

/// Emitted when Tax Program deposits rewards via CPI.
///
/// This is the 71% yield portion from taxed swaps.
#[event]
pub struct RewardsDeposited {
    /// Amount of SOL deposited.
    pub amount: u64,

    /// New pending_rewards balance after deposit.
    pub new_pending: u64,

    /// Escrow vault pubkey (for monitoring/filtering).
    /// Enables off-chain dashboards to filter events per vault and verify
    /// the vault address matches expectations without additional RPC calls.
    pub escrow_vault: Pubkey,

    /// Escrow vault SOL balance at time of deposit (for reconciliation monitoring).
    /// If escrow_balance < new_pending, something is wrong. The on-chain require!
    /// already catches this and reverts, but the event gives visibility into
    /// healthy deposits too.
    pub escrow_balance: u64,

    /// Slot when deposit occurred.
    pub slot: u64,
}

/// Emitted when Epoch Program updates cumulative via CPI.
///
/// Signals that pending rewards have been added to the global accumulator.
#[event]
pub struct CumulativeUpdated {
    /// The epoch number that was finalized.
    pub epoch: u32,

    /// Amount of SOL added to cumulative (was pending_rewards).
    pub rewards_added: u64,

    /// New rewards_per_token_stored value.
    pub new_cumulative: u128,

    /// Total staked at time of update (denominator for calculation).
    pub total_staked: u64,

    /// Slot when update occurred.
    pub slot: u64,
}
