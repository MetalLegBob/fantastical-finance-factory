//! Dr. Fraudsworth Staking Program
//!
//! This program implements PROFIT token staking for SOL yield distribution.
//! Users stake PROFIT to earn a share of the protocol's SOL revenue, distributed
//! pro-rata using the battle-tested cumulative reward-per-token pattern (Synthetix/Quarry).
//!
//! Key features:
//! - Instant unstake (no lockup period)
//! - Separate claim instruction (no forced unstake to harvest)
//! - First-depositor attack mitigation via MINIMUM_STAKE dead stake
//! - Flash loan resistant (stake/unstake same epoch = zero rewards)
//!
//! Instructions:
//! - `initialize_stake_pool`: Create global state (once at deployment)
//! - `stake`: Stake PROFIT tokens to begin earning
//! - `unstake`: Withdraw staked PROFIT + claim pending rewards
//! - `claim`: Claim pending SOL rewards without unstaking
//! - `deposit_rewards`: Called by Tax Program to deposit yield portion
//! - `update_cumulative`: Called by Epoch Program to finalize epoch rewards
//!
//! Source: Docs/New_Yield_System_Spec.md

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod helpers;
pub mod instructions;
pub mod state;

use instructions::*;

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Dr Fraudsworth's Finance Factory",
    project_url: "https://fraudsworth.fun",
    contacts: "email:drfraudsworth@gmail.com,twitter:@fraudsworth",
    policy: "https://fraudsworth.fun/docs/security/security-policy",
    preferred_languages: "en",
    auditors: "Internal audits: SOS, BOK, VulnHunter (v1.3)",
    expiry: "2027-03-20"
}

#[cfg(all(not(feature = "no-entrypoint"), feature = "devnet"))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_STAKING: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC|tax=5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ|epoch=64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY|profit=2t1K59GE2b4oXSRbZCyJo7VE3DXSJmtVBkE5kdrT3xLW";
#[cfg(all(not(feature = "no-entrypoint"), not(feature = "devnet")))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_STAKING: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH|tax=43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj|epoch=4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2|profit=pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR";

#[cfg(feature = "devnet")]
declare_id!("3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC");
#[cfg(not(feature = "devnet"))]
declare_id!("12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH");

#[program]
pub mod staking {
    use super::*;

    /// Initialize the global stake pool with dead stake.
    ///
    /// Creates StakePool, EscrowVault, and StakeVault PDAs.
    /// Transfers MINIMUM_STAKE (1 PROFIT) as dead stake to prevent
    /// first-depositor attack.
    ///
    /// Can only be called once (Anchor's init constraint prevents re-init).
    pub fn initialize_stake_pool<'info>(
        ctx: Context<'_, '_, 'info, 'info, InitializeStakePool<'info>>,
    ) -> Result<()> {
        instructions::initialize_stake_pool::handler(ctx)
    }

    /// Stake PROFIT tokens to begin earning yield.
    ///
    /// Transfers PROFIT from user to stake vault.
    /// Creates UserStake account if first stake.
    /// Updates rewards checkpoint before balance change.
    pub fn stake<'info>(
        ctx: Context<'_, '_, 'info, 'info, Stake<'info>>,
        amount: u64,
    ) -> Result<()> {
        instructions::stake::handler(ctx, amount)
    }

    /// Unstake PROFIT tokens and auto-claim pending rewards.
    ///
    /// Transfers PROFIT from stake vault to user.
    /// Automatically claims any pending SOL rewards.
    /// If partial unstake would leave < MINIMUM_STAKE, does full unstake.
    pub fn unstake<'info>(
        ctx: Context<'_, '_, 'info, 'info, Unstake<'info>>,
        amount: u64,
    ) -> Result<()> {
        instructions::unstake::handler(ctx, amount)
    }

    /// Claim pending SOL rewards without unstaking.
    ///
    /// Transfers accumulated SOL rewards from escrow to user.
    /// User's staked_balance remains unchanged.
    /// Fails if no rewards to claim.
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        instructions::claim::handler(ctx)
    }

    /// Deposit SOL rewards (called by Tax Program via CPI).
    ///
    /// Increments pending_rewards counter. SOL already transferred by caller.
    /// Access restricted to Tax Program via seeds::program constraint.
    ///
    /// # Arguments
    /// * `amount` - Amount of SOL deposited in lamports
    pub fn deposit_rewards(ctx: Context<DepositRewards>, amount: u64) -> Result<()> {
        instructions::deposit_rewards::handler(ctx, amount)
    }

    /// Finalize epoch rewards (called by Epoch Program via CPI).
    ///
    /// Moves pending_rewards to cumulative rewards_per_token_stored.
    /// Access restricted to Epoch Program via seeds::program constraint.
    ///
    /// # Arguments
    /// * `epoch` - The epoch number being finalized
    pub fn update_cumulative(ctx: Context<UpdateCumulative>, epoch: u32) -> Result<()> {
        instructions::update_cumulative::handler(ctx, epoch)
    }

    /// Test-only: deposit SOL rewards and distribute in one step.
    /// Bypasses Tax/Epoch CPI gating for unit test convenience.
    /// Only exists when built with `--features test`.
    #[cfg(feature = "test")]
    pub fn test_deposit_and_distribute(
        ctx: Context<TestDepositAndDistribute>,
        amount: u64,
    ) -> Result<()> {
        instructions::test_helpers::handler(ctx, amount)
    }
}
