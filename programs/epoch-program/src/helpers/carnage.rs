//! Carnage VRF byte interpretation helpers.
//!
//! Derives Carnage trigger, action, and target from VRF bytes 5-7.
//! (Shifted from bytes 3-5 in Phase 37 to accommodate independent tax rolls.)
//!
//! VRF byte allocation:
//! - Bytes 0-4: Tax derivation (flip + 4 magnitude rolls)
//! - Byte 5: Carnage trigger
//! - Byte 6: Carnage action
//! - Byte 7: Carnage target
//!
//! Source: Carnage_Fund_Spec.md Sections 6-7

use crate::constants::{CARNAGE_SELL_THRESHOLD, CARNAGE_TRIGGER_THRESHOLD};
use crate::state::{CarnageAction, Token};

/// Check if Carnage is triggered from VRF byte 5.
///
/// Returns true if byte 5 < CARNAGE_TRIGGER_THRESHOLD (11).
/// Probability: 11/256 = ~4.3% = ~1/24 epochs
/// Expected frequency: ~2 triggers per day (48 epochs/day)
///
/// Source: Carnage_Fund_Spec.md Section 7.1
#[inline]
pub fn is_carnage_triggered(vrf_result: &[u8; 32]) -> bool {
    vrf_result[5] < CARNAGE_TRIGGER_THRESHOLD
}

/// Determine Carnage action from VRF byte 6.
///
/// - If no holdings: returns CarnageAction::None (BuyOnly path)
/// - If byte 6 < CARNAGE_SELL_THRESHOLD (5): returns Sell (2% probability)
/// - Else: returns Burn (98% probability)
///
/// Source: Carnage_Fund_Spec.md Section 7.2
pub fn get_carnage_action(vrf_result: &[u8; 32], has_holdings: bool) -> CarnageAction {
    if !has_holdings {
        return CarnageAction::None;
    }

    if vrf_result[6] < CARNAGE_SELL_THRESHOLD {
        CarnageAction::Sell
    } else {
        CarnageAction::Burn
    }
}

/// Determine Carnage buy target from VRF byte 7.
///
/// - If byte 7 < 128: returns Token::Crime (50%)
/// - Else: returns Token::Fraud (50%)
///
/// Note: Target is VRF-determined regardless of which token is currently held.
/// This means after a 2% sell, Carnage could immediately rebuy the same token.
///
/// Source: Carnage_Fund_Spec.md Section 7.3
#[inline]
pub fn get_carnage_target(vrf_result: &[u8; 32]) -> Token {
    if vrf_result[7] < 128 {
        Token::Crime
    } else {
        Token::Fraud
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a 32-byte VRF result with Carnage bytes at positions 5, 6, 7.
    fn make_vrf(byte5: u8, byte6: u8, byte7: u8) -> [u8; 32] {
        let mut result = [0u8; 32];
        result[5] = byte5;
        result[6] = byte6;
        result[7] = byte7;
        result
    }

    // Trigger tests (byte 5)
    #[test]
    fn test_carnage_triggers_below_threshold() {
        for i in 0..11u8 {
            let vrf = make_vrf(i, 0, 0);
            assert!(is_carnage_triggered(&vrf), "byte5={} should trigger", i);
        }
    }

    #[test]
    fn test_carnage_no_trigger_at_threshold() {
        let vrf = make_vrf(11, 0, 0);
        assert!(!is_carnage_triggered(&vrf));
    }

    #[test]
    fn test_carnage_no_trigger_above_threshold() {
        for i in 11..=255u8 {
            let vrf = make_vrf(i, 0, 0);
            assert!(
                !is_carnage_triggered(&vrf),
                "byte5={} should not trigger",
                i
            );
        }
    }

    // Action tests (byte 6)
    #[test]
    fn test_action_none_when_no_holdings() {
        let vrf = make_vrf(0, 0, 0);
        assert_eq!(get_carnage_action(&vrf, false), CarnageAction::None);
    }

    #[test]
    fn test_action_sell_below_threshold() {
        for i in 0..5u8 {
            let vrf = make_vrf(0, i, 0);
            assert_eq!(
                get_carnage_action(&vrf, true),
                CarnageAction::Sell,
                "byte6={} with holdings should be Sell",
                i
            );
        }
    }

    #[test]
    fn test_action_burn_at_and_above_threshold() {
        for i in 5..=255u8 {
            let vrf = make_vrf(0, i, 0);
            assert_eq!(
                get_carnage_action(&vrf, true),
                CarnageAction::Burn,
                "byte6={} with holdings should be Burn",
                i
            );
        }
    }

    // Target tests (byte 7)
    #[test]
    fn test_target_crime_below_128() {
        for i in 0..128u8 {
            let vrf = make_vrf(0, 0, i);
            assert_eq!(
                get_carnage_target(&vrf),
                Token::Crime,
                "byte7={} should target CRIME",
                i
            );
        }
    }

    #[test]
    fn test_target_fraud_at_and_above_128() {
        for i in 128..=255u8 {
            let vrf = make_vrf(0, 0, i);
            assert_eq!(
                get_carnage_target(&vrf),
                Token::Fraud,
                "byte7={} should target FRAUD",
                i
            );
        }
    }

    // Probability distribution tests (documented, not enforced)
    #[test]
    fn test_trigger_probability_documented() {
        // 11/256 = 4.296875% ≈ 4.3%
        assert_eq!(CARNAGE_TRIGGER_THRESHOLD, 11);
    }

    #[test]
    fn test_sell_probability_documented() {
        // 5/256 = 1.953125% ≈ 2%
        assert_eq!(CARNAGE_SELL_THRESHOLD, 5);
    }
}
