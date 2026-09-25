//! Constants for the staking program.
//!
//! This module defines all magic numbers, precision factors, and PDA seeds
//! used throughout the staking system. Following DeFi conventions:
//! - PRECISION uses 1e18 for maximum accuracy in reward calculations
//! - MINIMUM_STAKE prevents first-depositor inflation attacks
//!
//! Source: Docs/New_Yield_System_Spec.md Section 4

use anchor_lang::prelude::Pubkey;
use anchor_lang::pubkey;

/// Precision multiplier for reward calculations.
///
/// 1e18 is the DeFi standard (matching Solidity's 18 decimal places).
/// This provides ~18 decimal places of precision for the cumulative
/// reward-per-token calculation, preventing significant rounding errors
/// even over long timeframes.
///
/// Formula context: `pending = (balance * delta) / PRECISION`
pub const PRECISION: u128 = 1_000_000_000_000_000_000;

/// Minimum stake to prevent first-depositor attack.
///
/// The protocol stakes this amount (1 PROFIT) during initialization,
/// creating a "dead stake" that ensures:
/// 1. Attacker cannot be the first depositor
/// 2. There's always a meaningful denominator for reward math
/// 3. Maximum manipulation requires donating >1M SOL (economically infeasible)
///
/// 1 PROFIT = 1,000,000 units (6 decimals)
pub const MINIMUM_STAKE: u64 = 1_000_000;

/// PROFIT token decimals.
///
/// Used for transfer_checked CPI calls which require decimal specification.
pub const PROFIT_DECIMALS: u8 = 6;

/// Canonical PROFIT mint for the selected deployment cluster.
///
/// The initializer must never accept a merely PROFIT-shaped mint: all stake
/// accounting and downstream Conversion/Vote integrations refer to this exact
/// cluster identity.
#[cfg(feature = "devnet")]
pub fn canonical_profit_mint() -> Pubkey {
    pubkey!("2t1K59GE2b4oXSRbZCyJo7VE3DXSJmtVBkE5kdrT3xLW")
}

#[cfg(not(feature = "devnet"))]
pub fn canonical_profit_mint() -> Pubkey {
    pubkey!("pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR")
}

/// Cooldown period after claiming before unstake is allowed.
///
/// Users must wait this many seconds after their last claim
/// before they can unstake. Discourages mercenary capital
/// without being punitive.
///
/// 43,200 seconds = 12 hours
#[cfg(not(feature = "test"))]
pub const COOLDOWN_SECONDS: i64 = 43_200;
#[cfg(feature = "test")]
pub const COOLDOWN_SECONDS: i64 = 2;

/// PDA seed for the global StakePool account.
/// Seeds: ["stake_pool"]
pub const STAKE_POOL_SEED: &[u8] = b"stake_pool";

/// PDA seed for per-user UserStake accounts.
/// Seeds: ["user_stake", user_pubkey]
pub const USER_STAKE_SEED: &[u8] = b"user_stake";

/// PDA seed for the SOL escrow vault.
/// Seeds: ["escrow_vault"]
/// This account holds undistributed SOL yield.
pub const ESCROW_VAULT_SEED: &[u8] = b"escrow_vault";

/// PDA seed for the PROFIT stake vault.
/// Seeds: ["stake_vault"]
/// This account holds all staked PROFIT tokens.
pub const STAKE_VAULT_SEED: &[u8] = b"stake_vault";

/// Epoch Program's staking authority PDA seed.
///
/// CRITICAL: This must match Epoch Program's derivation exactly.
/// Used for seeds::program constraint to verify CPI caller.
/// If mismatched, update_cumulative will reject all Epoch Program calls.
///
/// Cross-program dependency: epoch_program::STAKING_AUTHORITY_SEED
pub const STAKING_AUTHORITY_SEED: &[u8] = b"staking_authority";

/// Tax Program's tax authority PDA seed.
///
/// CRITICAL: This must match Tax Program's derivation exactly.
/// Used for seeds::program constraint to verify CPI caller.
/// If mismatched, deposit_rewards will reject all Tax Program calls.
///
/// Cross-program dependency: Must match tax_program::TAX_AUTHORITY_SEED.
pub const TAX_AUTHORITY_SEED: &[u8] = b"tax_authority";

/// Returns the Tax Program ID for cross-program PDA verification.
///
/// This must match the declare_id! in Tax Program (lib.rs).
/// Used in seeds::program constraint to verify CPI caller.
///
/// DEPLOYMENT CHECKLIST:
/// 1. Deploy Tax Program
/// 2. Get program ID from deploy output
/// 3. Update this function with actual program ID
/// 4. Redeploy Staking Program
#[cfg(feature = "devnet")]
pub fn tax_program_id() -> Pubkey {
    pubkey!("5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ")
}

#[cfg(not(feature = "devnet"))]
pub fn tax_program_id() -> Pubkey {
    // Tax Program ID from programs/tax-program/src/lib.rs declare_id!
    pubkey!("43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj")
}

/// Returns the Epoch Program ID for cross-program PDA verification.
///
/// This must match the declare_id! in Epoch Program (lib.rs).
/// Used in seeds::program constraint to verify CPI caller.
///
/// DEPLOYMENT CHECKLIST:
/// 1. Deploy Epoch Program
/// 2. Get program ID from deploy output
/// 3. Update this function with actual program ID
/// 4. Redeploy Staking Program
#[cfg(feature = "devnet")]
pub fn epoch_program_id() -> Pubkey {
    pubkey!("64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY")
}

#[cfg(not(feature = "devnet"))]
pub fn epoch_program_id() -> Pubkey {
    // Epoch Program ID from programs/epoch-program/src/lib.rs declare_id!
    pubkey!("4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2")
}

/// Anchor discriminator for Staking::deposit_rewards instruction.
/// Computed: sha256("global:deposit_rewards")[0..8]
/// Used by Tax Program when building CPI instruction data.
pub const DEPOSIT_REWARDS_DISCRIMINATOR: [u8; 8] = [52, 249, 112, 72, 206, 161, 196, 1];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tax_authority_seed() {
        assert_eq!(TAX_AUTHORITY_SEED, b"tax_authority");
        assert_eq!(TAX_AUTHORITY_SEED.len(), 13);
    }

    #[test]
    fn test_tax_program_id() {
        // Verify the pubkey is valid and matches expected
        let id = tax_program_id();
        #[cfg(not(feature = "devnet"))]
        assert_eq!(
            id.to_string(),
            "43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj"
        );
        #[cfg(feature = "devnet")]
        assert_eq!(
            id.to_string(),
            "5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ"
        );
    }

    /// Test that epoch_program_id returns the correct Epoch Program ID.
    ///
    /// This verifies:
    /// 1. The ID is not a placeholder (all 1s)
    /// 2. The ID matches what's in epoch-program/src/lib.rs
    #[test]
    fn test_epoch_program_id() {
        let id = epoch_program_id();

        // Verify it's not a placeholder
        assert_ne!(
            id.to_string(),
            "11111111111111111111111111111111",
            "epoch_program_id should not be System Program placeholder"
        );

        // Verify it matches the expected Epoch Program ID
        #[cfg(not(feature = "devnet"))]
        assert_eq!(
            id.to_string(),
            "4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2",
            "epoch_program_id must match Epoch Program declare_id!"
        );
        #[cfg(feature = "devnet")]
        assert_eq!(
            id.to_string(),
            "64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY",
            "epoch_program_id must match Epoch Program devnet declare_id!"
        );
    }

    /// Test that MINIMUM_STAKE is 1 PROFIT (1M units with 6 decimals).
    #[test]
    fn test_minimum_stake() {
        assert_eq!(MINIMUM_STAKE, 1_000_000);
    }

    #[test]
    fn test_canonical_profit_mint() {
        #[cfg(not(feature = "devnet"))]
        assert_eq!(
            canonical_profit_mint().to_string(),
            "pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR"
        );
        #[cfg(feature = "devnet")]
        assert_eq!(
            canonical_profit_mint().to_string(),
            "2t1K59GE2b4oXSRbZCyJo7VE3DXSJmtVBkE5kdrT3xLW"
        );
    }

    /// Test that PRECISION is 1e18 (DeFi standard).
    #[test]
    fn test_precision() {
        assert_eq!(PRECISION, 1_000_000_000_000_000_000);
    }

    /// Test that staking authority seed matches expected value.
    #[test]
    fn test_staking_authority_seed() {
        assert_eq!(STAKING_AUTHORITY_SEED, b"staking_authority");
        assert_eq!(STAKING_AUTHORITY_SEED.len(), 17);
    }

    /// Verify DEPOSIT_REWARDS_DISCRIMINATOR matches sha256("global:deposit_rewards")[0..8].
    ///
    /// This discriminator is used by Tax Program when building CPI instruction data
    /// to call deposit_rewards on the Staking Program.
    #[test]
    fn test_deposit_rewards_discriminator() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"global:deposit_rewards");
        let result = hasher.finalize();
        let expected: [u8; 8] = result[0..8].try_into().unwrap();
        assert_eq!(
            DEPOSIT_REWARDS_DISCRIMINATOR, expected,
            "Discriminator mismatch: expected {:02x?}, got {:02x?}",
            expected, DEPOSIT_REWARDS_DISCRIMINATOR
        );
    }
}
