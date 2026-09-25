//! Error codes for the staking program.
//!
//! This module defines all error variants following the ERR-XX naming
//! convention from the spec. Errors are organized by category:
//! - Validation errors (input checking)
//! - Authorization errors (access control)
//! - Arithmetic errors (math safety)
//! - State errors (invalid state transitions)
//!
//! Source: Docs/New_Yield_System_Spec.md Section 11

use anchor_lang::prelude::*;

/// Staking program error codes.
///
/// Error codes are assigned stable offsets for client compatibility.
/// Adding new errors should append to existing categories.
#[error_code]
pub enum StakingError {
    // =========================================================================
    // Validation Errors (ERR-01, ERR-02)
    // =========================================================================
    /// ERR-01: Amount must be greater than zero.
    ///
    /// Triggered when stake/unstake/deposit_rewards receives amount=0.
    /// Zero-amount operations waste compute and indicate a logic bug.
    #[msg("Amount must be greater than zero")]
    ZeroAmount,

    /// ERR-02: Insufficient staked balance.
    ///
    /// Triggered when unstake amount exceeds user's staked_balance.
    /// Prevents underflow in balance subtraction.
    #[msg("Insufficient staked balance")]
    InsufficientBalance,

    /// ERR-03: Insufficient SOL in escrow vault.
    ///
    /// Triggered when claim/unstake reward transfer exceeds escrow balance.
    /// This should never happen if deposit_rewards and update_cumulative
    /// are called correctly, but protects against edge cases.
    #[msg("Insufficient SOL in escrow vault")]
    InsufficientEscrowBalance,

    /// ERR-04: No rewards to claim.
    ///
    /// Triggered when user calls claim with rewards_earned=0.
    /// After update_rewards, if no delta exists, nothing to claim.
    #[msg("No rewards to claim")]
    NothingToClaim,

    // =========================================================================
    // Authorization Errors (ERR-05)
    // =========================================================================
    /// ERR-05: Unauthorized access.
    ///
    /// Triggered when signer does not own the UserStake account.
    /// Prevents users from claiming/unstaking another user's position.
    #[msg("Unauthorized: signer does not own this stake account")]
    Unauthorized,

    // =========================================================================
    // Arithmetic Errors
    // =========================================================================
    /// Arithmetic overflow.
    ///
    /// Triggered when checked_add/mul returns None.
    /// All arithmetic uses checked operations to fail safely.
    #[msg("Arithmetic overflow")]
    Overflow,

    /// Arithmetic underflow.
    ///
    /// Triggered when checked_sub returns None.
    /// Prevents wrapping on subtraction operations.
    #[msg("Arithmetic underflow")]
    Underflow,

    /// Division by zero.
    ///
    /// Triggered when checked_div returns None.
    /// Prevented by dead stake (MINIMUM_STAKE) in normal operation.
    #[msg("Division by zero")]
    DivisionByZero,

    // =========================================================================
    // State Errors
    // =========================================================================
    /// Cumulative already updated for this epoch.
    ///
    /// Triggered when update_cumulative is called with epoch <= last_update_epoch.
    /// Prevents double-distribution of rewards within same epoch.
    #[msg("Cumulative already updated for this epoch")]
    AlreadyUpdated,

    /// Pool not initialized.
    ///
    /// Triggered when operating on uninitialized StakePool.
    /// initialize_stake_pool must be called first.
    #[msg("Pool not initialized")]
    NotInitialized,

    /// Pool already initialized.
    ///
    /// Triggered when initialize_stake_pool is called twice.
    /// Each pool PDA can only be initialized once.
    #[msg("Pool already initialized")]
    AlreadyInitialized,

    /// Cooldown period has not elapsed since last claim.
    ///
    /// Users must wait COOLDOWN_SECONDS (12 hours) after claiming
    /// before they can unstake. This prevents mercenary capital
    /// from extracting rewards and immediately exiting.
    #[msg("Cooldown active: must wait 12 hours after claiming before unstaking")]
    CooldownActive,

    /// Initialization must use the cluster's canonical PROFIT mint.
    #[msg("Mint is not the canonical PROFIT mint for this cluster")]
    InvalidProfitMint,

    /// PROFIT is a Token-2022 mint and must never be routed through classic
    /// SPL Token or an arbitrary program.
    #[msg("PROFIT requires the Token-2022 program")]
    InvalidTokenProgram,

    /// Staking's raw-unit constants assume six-decimal PROFIT.
    #[msg("Canonical PROFIT mint must have six decimals")]
    InvalidProfitDecimals,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializer_identity_error_codes_are_append_only() {
        use anchor_lang::error::ERROR_CODE_OFFSET;

        assert_eq!(
            StakingError::InvalidProfitMint as u32 + ERROR_CODE_OFFSET,
            6012
        );
        assert_eq!(
            StakingError::InvalidTokenProgram as u32 + ERROR_CODE_OFFSET,
            6013
        );
        assert_eq!(
            StakingError::InvalidProfitDecimals as u32 + ERROR_CODE_OFFSET,
            6014
        );
    }
}
