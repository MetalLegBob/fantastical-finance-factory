//! CarnageFundState account structure.
//!
//! Tracks the Carnage Fund's holdings, vaults, and lifetime statistics.
//!
//! Source: Carnage_Fund_Spec.md Section 4

use anchor_lang::prelude::*;

/// Token currently held by the Carnage Fund.
///
/// Used for type-safe operations on held_token field.
/// Stored as u8 in CarnageFundState (0 = None, 1 = Crime, 2 = Fraud).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum HeldToken {
    /// No token held (SOL only)
    None = 0,
    /// CRIME token held
    Crime = 1,
    /// FRAUD token held
    Fraud = 2,
}

impl HeldToken {
    /// Convert from u8 representation.
    /// Returns None for invalid values.
    pub fn from_u8(value: u8) -> Option<HeldToken> {
        match value {
            0 => Some(HeldToken::None),
            1 => Some(HeldToken::Crime),
            2 => Some(HeldToken::Fraud),
            _ => None,
        }
    }

    /// Convert to u8 representation.
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Global Carnage Fund state account.
///
/// Single PDA: seeds = ["carnage_fund"]
///
/// This account tracks the Carnage Fund's holdings, vault addresses,
/// and lifetime statistics. The Carnage Fund accumulates SOL from protocol
/// fees and uses it for random buyback-and-burn operations.
///
/// **Size calculation:**
/// - Discriminator: 8 bytes
/// - sol_vault: 32 bytes
/// - crime_vault: 32 bytes
/// - fraud_vault: 32 bytes
/// - held_token: 1 byte
/// - held_amount: 8 bytes
/// - last_trigger_epoch: 4 bytes
/// - total_sol_spent: 8 bytes
/// - total_crime_burned: 8 bytes
/// - total_fraud_burned: 8 bytes
/// - total_triggers: 4 bytes
/// - initialized: 1 byte
/// - bump: 1 byte
/// Total: 8 + 139 = 147 bytes
///
/// Source: Carnage_Fund_Spec.md Section 4
#[account]
pub struct CarnageFundState {
    // =========================================================================
    // Vault PDAs (96 bytes)
    // =========================================================================
    /// PDA of the SOL vault (SystemAccount holding native SOL).
    /// Seeds = ["carnage_sol_vault"]
    pub sol_vault: Pubkey,

    /// PDA of the CRIME token vault (Token-2022 account).
    /// Seeds = ["carnage_crime_vault"]
    pub crime_vault: Pubkey,

    /// PDA of the FRAUD token vault (Token-2022 account).
    /// Seeds = ["carnage_fraud_vault"]
    pub fraud_vault: Pubkey,

    // =========================================================================
    // Current Holdings (13 bytes)
    // =========================================================================
    /// Which token is currently held (0=None, 1=CRIME, 2=FRAUD).
    /// Use u8 to avoid Borsh enum serialization complexity.
    /// See HeldToken enum for type-safe operations.
    pub held_token: u8,

    /// Amount of held token (0 if held_token = None).
    /// Represents tokens purchased during Carnage trigger,
    /// waiting for next Carnage to burn or sell.
    pub held_amount: u64,

    // =========================================================================
    // Timing (4 bytes)
    // =========================================================================
    /// Last epoch when Carnage triggered.
    /// Used to track Carnage frequency for analytics.
    pub last_trigger_epoch: u32,

    // =========================================================================
    // Lifetime Statistics (28 bytes)
    // =========================================================================
    /// Lifetime statistics: total SOL spent on buys (in lamports).
    /// Monotonically increasing.
    pub total_sol_spent: u64,

    /// Lifetime statistics: total CRIME burned.
    /// Monotonically increasing.
    pub total_crime_burned: u64,

    /// Lifetime statistics: total FRAUD burned.
    /// Monotonically increasing.
    pub total_fraud_burned: u64,

    /// Lifetime statistics: total triggers executed.
    /// Monotonically increasing counter.
    pub total_triggers: u32,

    // =========================================================================
    // Protocol (2 bytes)
    // =========================================================================
    /// Initialization flag.
    /// Set to true in initialize_carnage_fund, prevents re-initialization.
    pub initialized: bool,

    /// PDA bump seed.
    pub bump: u8,
}

impl CarnageFundState {
    /// Total account size including 8-byte discriminator.
    /// 8 (discriminator) + 139 (data) = 147 bytes.
    ///
    /// Source: Carnage_Fund_Spec.md Section 4
    pub const LEN: usize = 8 + 139;

    /// Calculate data size without discriminator (for verification).
    /// Should equal 139 bytes.
    ///
    /// 3 Pubkeys: 3 * 32 = 96
    /// held_token: 1
    /// held_amount: 8
    /// last_trigger_epoch: 4
    /// total_sol_spent: 8
    /// total_crime_burned: 8
    /// total_fraud_burned: 8
    /// total_triggers: 4
    /// initialized: 1
    /// bump: 1
    /// Total: 139
    pub const DATA_LEN: usize = 32 + 32 + 32 + 1 + 8 + 4 + 8 + 8 + 8 + 4 + 1 + 1;
}

// Static assertion that DATA_LEN equals expected 139 bytes
const _: () = assert!(CarnageFundState::DATA_LEN == 139);

// Static assertion that LEN equals discriminator + data
const _: () = assert!(CarnageFundState::LEN == 8 + CarnageFundState::DATA_LEN);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_carnage_fund_state_len() {
        assert_eq!(CarnageFundState::LEN, 147);
        assert_eq!(CarnageFundState::DATA_LEN, 139);
    }

    #[test]
    fn test_held_token_to_u8() {
        assert_eq!(HeldToken::None.to_u8(), 0);
        assert_eq!(HeldToken::Crime.to_u8(), 1);
        assert_eq!(HeldToken::Fraud.to_u8(), 2);
    }

    #[test]
    fn test_held_token_from_u8() {
        assert_eq!(HeldToken::from_u8(0), Some(HeldToken::None));
        assert_eq!(HeldToken::from_u8(1), Some(HeldToken::Crime));
        assert_eq!(HeldToken::from_u8(2), Some(HeldToken::Fraud));
        assert_eq!(HeldToken::from_u8(3), None);
        assert_eq!(HeldToken::from_u8(255), None);
    }

    #[test]
    fn test_held_token_roundtrip() {
        assert_eq!(
            HeldToken::from_u8(HeldToken::None.to_u8()),
            Some(HeldToken::None)
        );
        assert_eq!(
            HeldToken::from_u8(HeldToken::Crime.to_u8()),
            Some(HeldToken::Crime)
        );
        assert_eq!(
            HeldToken::from_u8(HeldToken::Fraud.to_u8()),
            Some(HeldToken::Fraud)
        );
    }
}
