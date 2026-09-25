//! VRF byte parsing for tax derivation.
//!
//! Converts VRF randomness bytes into discrete tax rates per Epoch_State_Machine_Spec.md Section 7.3.
//!
//! Key properties:
//! - 75% flip probability (byte 0 < 192)
//! - 4 discrete low rates: 100, 200, 300, 400 bps (1-4%)
//! - 4 discrete high rates: 1100, 1200, 1300, 1400 bps (11-14%)
//! - Independent CRIME and FRAUD magnitude rolls (4 VRF bytes)
//! - No intermediate values possible
//!
//! VRF byte allocation (Phase 37):
//! - Byte 0: Flip decision (< 192 = 75%)
//! - Byte 1: CRIME low tax magnitude (% 4 -> 100-400 bps)
//! - Byte 2: CRIME high tax magnitude (% 4 -> 1100-1400 bps)
//! - Byte 3: FRAUD low tax magnitude (% 4 -> 100-400 bps)
//! - Byte 4: FRAUD high tax magnitude (% 4 -> 1100-1400 bps)
//! - Bytes 5-7: Carnage (trigger, action, target)

use crate::state::Token;

/// Discrete low tax rates in basis points (1%, 2%, 3%, 4%).
const LOW_RATES: [u16; 4] = [100, 200, 300, 400];

/// Discrete high tax rates in basis points (11%, 12%, 13%, 14%).
const HIGH_RATES: [u16; 4] = [1100, 1200, 1300, 1400];

/// Threshold for flip decision: byte < 192 means flip (75% probability).
/// 192/256 = 0.75
const FLIP_THRESHOLD: u8 = 192;

/// Tax configuration derived from VRF randomness.
///
/// Contains the new cheap side and all four independently derived tax rates.
/// Legacy fields (low_tax_bps, high_tax_bps) are set to 0 since rates are
/// now independent per token -- there is no single "low" or "high" rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxConfig {
    /// Which token is cheap (low buy tax, high sell tax)
    pub cheap_side: Token,
    /// Legacy field -- set to 0. Use per-token rates instead.
    pub low_tax_bps: u16,
    /// Legacy field -- set to 0. Use per-token rates instead.
    pub high_tax_bps: u16,
    /// CRIME buy tax in basis points
    pub crime_buy_tax_bps: u16,
    /// CRIME sell tax in basis points
    pub crime_sell_tax_bps: u16,
    /// FRAUD buy tax in basis points
    pub fraud_buy_tax_bps: u16,
    /// FRAUD sell tax in basis points
    pub fraud_sell_tax_bps: u16,
}

/// Derives tax configuration from VRF randomness bytes.
///
/// Uses VRF bytes 0-4 for independent CRIME and FRAUD magnitude rolls:
/// - Byte 0: Flip decision (< 192 = 75% flip)
/// - Byte 1: CRIME low tax magnitude (% 4 -> 100/200/300/400 bps)
/// - Byte 2: CRIME high tax magnitude (% 4 -> 1100/1200/1300/1400 bps)
/// - Byte 3: FRAUD low tax magnitude (% 4 -> 100/200/300/400 bps)
/// - Byte 4: FRAUD high tax magnitude (% 4 -> 1100/1200/1300/1400 bps)
///
/// Assignment logic (each token gets its OWN independent rates):
/// - Cheap side: its OWN low buy / its OWN high sell
/// - Expensive side: its OWN high buy / its OWN low sell
///
/// # Arguments
/// * `vrf_result` - 32-byte VRF output (bytes 0-4 used)
/// * `current_cheap` - Current cheap side token before this epoch
///
/// # Returns
/// `TaxConfig` with independently derived per-token rates
///
/// # Example
/// ```ignore
/// let vrf_bytes = [128, 1, 2, 3, 0, ...]; // 32 bytes
/// let current = Token::Crime;
/// let config = derive_taxes(&vrf_bytes, current);
/// // Byte 0 (128) < 192 -> flip -> new cheap = Fraud
/// // CRIME: low=200, high=1300 -> expensive -> buy=1300, sell=200
/// // FRAUD: low=400, high=1100 -> cheap -> buy=400, sell=1100
/// ```
pub fn derive_taxes(vrf_result: &[u8; 32], current_cheap: Token) -> TaxConfig {
    // Byte 0: Flip decision
    let should_flip = vrf_result[0] < FLIP_THRESHOLD;

    // Bytes 1-2: CRIME magnitude rolls (independent)
    let crime_low_bps = LOW_RATES[(vrf_result[1] % 4) as usize];
    let crime_high_bps = HIGH_RATES[(vrf_result[2] % 4) as usize];

    // Bytes 3-4: FRAUD magnitude rolls (independent)
    let fraud_low_bps = LOW_RATES[(vrf_result[3] % 4) as usize];
    let fraud_high_bps = HIGH_RATES[(vrf_result[4] % 4) as usize];

    // Determine new cheap side
    let cheap_side = if should_flip {
        current_cheap.opposite()
    } else {
        current_cheap
    };

    // Assign rates: each token uses its OWN magnitude rolls
    // Cheap side: low buy, high sell (using that token's own low/high)
    // Expensive side: high buy, low sell (using that token's own high/low)
    let (crime_buy, crime_sell, fraud_buy, fraud_sell) = match cheap_side {
        Token::Crime => {
            // CRIME is cheap: CRIME gets low buy / high sell
            // FRAUD is expensive: FRAUD gets high buy / low sell
            (crime_low_bps, crime_high_bps, fraud_high_bps, fraud_low_bps)
        }
        Token::Fraud => {
            // FRAUD is cheap: FRAUD gets low buy / high sell
            // CRIME is expensive: CRIME gets high buy / low sell
            (crime_high_bps, crime_low_bps, fraud_low_bps, fraud_high_bps)
        }
    };

    TaxConfig {
        cheap_side,
        low_tax_bps: 0,  // Legacy -- rates are now independent per token
        high_tax_bps: 0, // Legacy -- rates are now independent per token
        crime_buy_tax_bps: crime_buy,
        crime_sell_tax_bps: crime_sell,
        fraud_buy_tax_bps: fraud_buy,
        fraud_sell_tax_bps: fraud_sell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a 32-byte VRF result with specified bytes 0-4
    /// (flip, crime_low, crime_high, fraud_low, fraud_high).
    fn make_vrf(byte0: u8, byte1: u8, byte2: u8, byte3: u8, byte4: u8) -> [u8; 32] {
        let mut result = [0u8; 32];
        result[0] = byte0;
        result[1] = byte1;
        result[2] = byte2;
        result[3] = byte3;
        result[4] = byte4;
        result
    }

    #[test]
    fn test_flip_at_threshold_boundary() {
        // Byte 0 = 191 (< 192) -> should flip
        let vrf_flip = make_vrf(191, 0, 0, 0, 0);
        let config = derive_taxes(&vrf_flip, Token::Crime);
        assert_eq!(config.cheap_side, Token::Fraud, "byte 191 should flip");

        // Byte 0 = 192 (>= 192) -> should NOT flip
        let vrf_no_flip = make_vrf(192, 0, 0, 0, 0);
        let config = derive_taxes(&vrf_no_flip, Token::Crime);
        assert_eq!(config.cheap_side, Token::Crime, "byte 192 should not flip");

        // Byte 0 = 0 (< 192) -> should flip
        let vrf_zero = make_vrf(0, 0, 0, 0, 0);
        let config = derive_taxes(&vrf_zero, Token::Crime);
        assert_eq!(config.cheap_side, Token::Fraud, "byte 0 should flip");

        // Byte 0 = 255 (>= 192) -> should NOT flip
        let vrf_max = make_vrf(255, 0, 0, 0, 0);
        let config = derive_taxes(&vrf_max, Token::Crime);
        assert_eq!(config.cheap_side, Token::Crime, "byte 255 should not flip");
    }

    #[test]
    fn test_all_crime_low_rates_reachable() {
        // Byte 1 values 0, 1, 2, 3 should map to CRIME low rates 100, 200, 300, 400 bps
        // When CRIME is cheap, crime_buy = crime_low
        for (byte_val, expected_bps) in [(0u8, 100u16), (1, 200), (2, 300), (3, 400)] {
            let vrf = make_vrf(255, byte_val, 0, 0, 0); // no flip, CRIME is cheap
            let config = derive_taxes(&vrf, Token::Crime);
            assert_eq!(
                config.crime_buy_tax_bps, expected_bps,
                "byte1={} should give CRIME buy {} bps",
                byte_val, expected_bps
            );
        }

        // Also test high byte values that wrap around
        for (byte_val, expected_bps) in [(252u8, 100u16), (253, 200), (254, 300), (255, 400)] {
            let vrf = make_vrf(255, byte_val, 0, 0, 0);
            let config = derive_taxes(&vrf, Token::Crime);
            assert_eq!(
                config.crime_buy_tax_bps, expected_bps,
                "byte1={} should give CRIME buy {} bps",
                byte_val, expected_bps
            );
        }
    }

    #[test]
    fn test_all_crime_high_rates_reachable() {
        // Byte 2 values 0, 1, 2, 3 should map to CRIME high rates 1100, 1200, 1300, 1400 bps
        // When CRIME is cheap, crime_sell = crime_high
        for (byte_val, expected_bps) in [(0u8, 1100u16), (1, 1200), (2, 1300), (3, 1400)] {
            let vrf = make_vrf(255, 0, byte_val, 0, 0); // no flip, CRIME is cheap
            let config = derive_taxes(&vrf, Token::Crime);
            assert_eq!(
                config.crime_sell_tax_bps, expected_bps,
                "byte2={} should give CRIME sell {} bps",
                byte_val, expected_bps
            );
        }

        // Also test high byte values that wrap around
        for (byte_val, expected_bps) in [(252u8, 1100u16), (253, 1200), (254, 1300), (255, 1400)] {
            let vrf = make_vrf(255, 0, byte_val, 0, 0);
            let config = derive_taxes(&vrf, Token::Crime);
            assert_eq!(
                config.crime_sell_tax_bps, expected_bps,
                "byte2={} should give CRIME sell {} bps",
                byte_val, expected_bps
            );
        }
    }

    #[test]
    fn test_all_fraud_low_rates_reachable() {
        // Byte 3 values 0, 1, 2, 3 should map to FRAUD low rates 100, 200, 300, 400 bps
        // When FRAUD is cheap, fraud_buy = fraud_low
        for (byte_val, expected_bps) in [(0u8, 100u16), (1, 200), (2, 300), (3, 400)] {
            let vrf = make_vrf(255, 0, 0, byte_val, 0); // no flip, FRAUD is cheap
            let config = derive_taxes(&vrf, Token::Fraud);
            assert_eq!(
                config.fraud_buy_tax_bps, expected_bps,
                "byte3={} should give FRAUD buy {} bps",
                byte_val, expected_bps
            );
        }
    }

    #[test]
    fn test_all_fraud_high_rates_reachable() {
        // Byte 4 values 0, 1, 2, 3 should map to FRAUD high rates 1100, 1200, 1300, 1400 bps
        // When FRAUD is cheap, fraud_sell = fraud_high
        for (byte_val, expected_bps) in [(0u8, 1100u16), (1, 1200), (2, 1300), (3, 1400)] {
            let vrf = make_vrf(255, 0, 0, 0, byte_val); // no flip, FRAUD is cheap
            let config = derive_taxes(&vrf, Token::Fraud);
            assert_eq!(
                config.fraud_sell_tax_bps, expected_bps,
                "byte4={} should give FRAUD sell {} bps",
                byte_val, expected_bps
            );
        }
    }

    #[test]
    fn test_rate_assignment_when_crime_is_cheap() {
        // When CRIME is cheap:
        // - CRIME: crime_low buy, crime_high sell (from bytes 1, 2)
        // - FRAUD: fraud_high buy, fraud_low sell (from bytes 4, 3)
        //
        // Bytes: flip=192(no), crime_low=1(200), crime_high=2(1300),
        //        fraud_low=3(400), fraud_high=0(1100)
        let vrf = make_vrf(192, 1, 2, 3, 0);
        let config = derive_taxes(&vrf, Token::Crime);

        assert_eq!(config.cheap_side, Token::Crime);
        assert_eq!(config.low_tax_bps, 0); // legacy
        assert_eq!(config.high_tax_bps, 0); // legacy
        assert_eq!(config.crime_buy_tax_bps, 200); // crime_low
        assert_eq!(config.crime_sell_tax_bps, 1300); // crime_high
        assert_eq!(config.fraud_buy_tax_bps, 1100); // fraud_high
        assert_eq!(config.fraud_sell_tax_bps, 400); // fraud_low
    }

    #[test]
    fn test_rate_assignment_when_fraud_is_cheap() {
        // When FRAUD is cheap:
        // - FRAUD: fraud_low buy, fraud_high sell (from bytes 3, 4)
        // - CRIME: crime_high buy, crime_low sell (from bytes 2, 1)
        //
        // Bytes: flip=192(no), crime_low=3(400), crime_high=0(1100),
        //        fraud_low=1(200), fraud_high=2(1300)
        let vrf = make_vrf(192, 3, 0, 1, 2);
        let config = derive_taxes(&vrf, Token::Fraud);

        assert_eq!(config.cheap_side, Token::Fraud);
        assert_eq!(config.low_tax_bps, 0); // legacy
        assert_eq!(config.high_tax_bps, 0); // legacy
        assert_eq!(config.crime_buy_tax_bps, 1100); // crime_high (expensive)
        assert_eq!(config.crime_sell_tax_bps, 400); // crime_low (expensive)
        assert_eq!(config.fraud_buy_tax_bps, 200); // fraud_low (cheap)
        assert_eq!(config.fraud_sell_tax_bps, 1300); // fraud_high (cheap)
    }

    #[test]
    fn test_independent_crime_fraud_rates() {
        // With independent rolls, CRIME and FRAUD can have different magnitudes.
        // This was impossible with shared bytes (mirrored rates).
        //
        // Bytes: flip=192(no), crime_low=0(100), crime_high=3(1400),
        //        fraud_low=2(300), fraud_high=1(1200)
        // CRIME is cheap -> CRIME: buy=100, sell=1400
        //                -> FRAUD: buy=1200, sell=300
        let vrf = make_vrf(192, 0, 3, 2, 1);
        let config = derive_taxes(&vrf, Token::Crime);

        assert_eq!(config.cheap_side, Token::Crime);
        // CRIME rates (cheap): low buy=100, high sell=1400
        assert_eq!(config.crime_buy_tax_bps, 100);
        assert_eq!(config.crime_sell_tax_bps, 1400);
        // FRAUD rates (expensive): high buy=1200, low sell=300
        assert_eq!(config.fraud_buy_tax_bps, 1200);
        assert_eq!(config.fraud_sell_tax_bps, 300);

        // Verify independence: CRIME low != FRAUD low, CRIME high != FRAUD high
        assert_ne!(
            config.crime_buy_tax_bps, config.fraud_sell_tax_bps,
            "Independent rolls should allow different low magnitudes"
        );
        assert_ne!(
            config.crime_sell_tax_bps, config.fraud_buy_tax_bps,
            "Independent rolls should allow different high magnitudes"
        );
    }

    #[test]
    fn test_legacy_fields_zero() {
        // Legacy low_tax_bps and high_tax_bps should always be 0
        // since rates are now independent per token.
        let vrf = make_vrf(192, 1, 2, 3, 0);
        let config = derive_taxes(&vrf, Token::Crime);
        assert_eq!(config.low_tax_bps, 0, "legacy low_tax_bps must be 0");
        assert_eq!(config.high_tax_bps, 0, "legacy high_tax_bps must be 0");

        // Also test with flip
        let vrf2 = make_vrf(0, 3, 0, 1, 2);
        let config2 = derive_taxes(&vrf2, Token::Crime);
        assert_eq!(
            config2.low_tax_bps, 0,
            "legacy low_tax_bps must be 0 after flip"
        );
        assert_eq!(
            config2.high_tax_bps, 0,
            "legacy high_tax_bps must be 0 after flip"
        );
    }
}
