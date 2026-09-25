//! Tax calculation and distribution split functions.
//!
//! Pure math functions operating on primitive types only.
//! No anchor_lang, no solana_program - keeps tests fast.
//!
//! Functions:
//! - calculate_tax: Compute tax amount from basis points
//! - split_distribution: Split total tax into (staking, carnage, treasury)
//!
//! All arithmetic uses checked_* operations and u128 intermediates.
//! Returns Option<T> -- never panics. None indicates overflow.
//!
//! Source: Tax_Pool_Logic_Spec.md, 18-RESEARCH.md

/// Calculate tax amount from a lamport value and tax rate in basis points.
///
/// Formula: `amount_lamports * tax_bps / 10_000`
///
/// Uses u128 intermediates to prevent overflow on large amounts.
///
/// # Arguments
/// * `amount_lamports` - Amount to tax (in lamports)
/// * `tax_bps` - Tax rate in basis points (e.g., 400 = 4%, 10000 = 100%)
///
/// # Returns
/// * `Some(tax)` - Calculated tax amount in lamports
/// * `None` - If tax_bps > 10000 (invalid rate) or arithmetic overflow
///
/// # Examples
/// - (1_000_000_000, 400) -> Some(40_000_000)  // 4% of 1 SOL
/// - (100, 400) -> Some(4)  // 4% of 100 lamports
/// - (10, 400) -> Some(0)  // Rounds down to 0
/// - (amount, 10001) -> None  // Invalid bps
pub fn calculate_tax(amount_lamports: u64, tax_bps: u16) -> Option<u64> {
    // Validate bps range: 0-10000 is valid (0% to 100%)
    if tax_bps > 10_000 {
        return None;
    }

    // Use u128 intermediates to prevent overflow
    let amount = amount_lamports as u128;
    let bps = tax_bps as u128;

    // tax = amount * tax_bps / 10_000
    // Multiply first for precision (no early truncation)
    let tax = amount.checked_mul(bps)?.checked_div(10_000)?;

    // Convert back to u64 - safe because:
    // max = u64::MAX * 10_000 / 10_000 = u64::MAX
    u64::try_from(tax).ok()
}

/// Split total tax into (staking, carnage, treasury) portions.
///
/// Distribution (71/24/5 split):
/// - Staking: 71% (floor) — 7100 bps
/// - Carnage: 24% (floor) — 2400 bps
/// - Treasury: remainder (absorbs rounding dust) — ~5%
///
/// Micro-tax edge case: If total_tax < 4 lamports, all goes to staking.
/// This avoids splitting dust across three destinations.
///
/// # Arguments
/// * `total_tax` - Total tax amount to distribute (in lamports)
///
/// # Returns
/// * `Some((staking, carnage, treasury))` - Distribution tuple
/// * `None` - If arithmetic underflows (shouldn't happen with valid inputs)
///
/// # Invariant
/// staking + carnage + treasury == total_tax (always)
///
/// # Examples
/// - (10000) -> Some((7100, 2400, 500))  // Clean 71/24/5 split
/// - (3) -> Some((3, 0, 0))  // Micro-tax: all to staking
/// - (0) -> Some((0, 0, 0))  // Zero tax
pub fn split_distribution(total_tax: u64) -> Option<(u64, u64, u64)> {
    // Distribution BPS constants — must match constants.rs STAKING_BPS/CARNAGE_BPS.
    // Defined inline to keep this module free of anchor_lang dependencies (fast tests).
    const STAKING_BPS: u128 = 7_100; // 71%
    const CARNAGE_BPS: u128 = 2_400; // 24%
    const BPS_DENOM: u128 = 10_000;

    // Handle micro-tax edge case: if tax < 4 lamports, all to staking
    // This avoids splitting dust across three destinations
    if total_tax < 4 {
        return Some((total_tax, 0, 0));
    }

    // Use u128 intermediates to prevent overflow
    let total = total_tax as u128;

    // Staking: floor(total * 7100 / 10000)
    let staking_u128 = total.checked_mul(STAKING_BPS)?.checked_div(BPS_DENOM)?;
    let staking = u64::try_from(staking_u128).ok()?;

    // Carnage: floor(total * 2400 / 10000)
    let carnage_u128 = total.checked_mul(CARNAGE_BPS)?.checked_div(BPS_DENOM)?;
    let carnage = u64::try_from(carnage_u128).ok()?;

    // Treasury: remainder (absorbs rounding dust)
    // This ensures the invariant: staking + carnage + treasury == total_tax
    let treasury = total_tax.checked_sub(staking)?.checked_sub(carnage)?;

    Some((staking, carnage, treasury))
}

/// Calculate protocol-enforced minimum output floor.
///
/// Uses the constant-product AMM formula to compute the expected output,
/// then applies a floor percentage (in basis points) to get the minimum
/// acceptable output.
///
/// Formula:
///   expected = reserve_out * amount_in / (reserve_in + amount_in)
///   floor = expected * floor_bps / 10_000
///
/// Uses u128 intermediate math to prevent overflow (same pattern as Carnage
/// slippage in Phase 47).
///
/// # Arguments
/// * `reserve_in` - Pool reserve of the input token
/// * `reserve_out` - Pool reserve of the output token
/// * `amount_in` - Amount being swapped into the pool
/// * `floor_bps` - Floor percentage in basis points (5000 = 50%)
///
/// # Returns
/// * `Some(floor)` - The minimum acceptable output amount
/// * `Some(0)` - If any input is 0 (empty pool edge case -- no floor enforceable)
/// * `None` - If arithmetic overflows (should not happen with u128 intermediates)
///
/// # Why no LP fee adjustment
/// At a 50% floor, the ~1% LP fee is absorbed. Raw constant-product is simpler
/// and the 50% threshold provides massive tolerance (see 49-RESEARCH.md Pitfall 4).
///
/// Source: Phase 49 (SEC-10), 49-RESEARCH.md Pattern 2
pub fn calculate_output_floor(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    floor_bps: u64,
) -> Option<u64> {
    // Empty pool or zero swap: no floor enforceable
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return Some(0);
    }

    // Constant-product expected output (no LP fee adjustment needed at 50% floor):
    // expected = reserve_out * amount_in / (reserve_in + amount_in)
    let numerator = (reserve_out as u128).checked_mul(amount_in as u128)?;
    let denominator = (reserve_in as u128).checked_add(amount_in as u128)?;
    let expected = numerator.checked_div(denominator)?;

    // Floor = expected * floor_bps / 10_000
    let floor = expected
        .checked_mul(floor_bps as u128)?
        .checked_div(10_000)?;

    // Defense-in-depth: checked cast (floor <= expected <= reserve_out <= u64::MAX)
    u64::try_from(floor).ok()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Part A: calculate_tax tests
    // =========================================================================

    #[test]
    fn tax_4pct_on_1_sol() {
        // 4% of 1 SOL (1_000_000_000 lamports) = 40_000_000 lamports
        assert_eq!(calculate_tax(1_000_000_000, 400), Some(40_000_000));
    }

    #[test]
    fn tax_14pct_on_1_sol() {
        // 14% of 1 SOL = 140_000_000 lamports
        assert_eq!(calculate_tax(1_000_000_000, 1400), Some(140_000_000));
    }

    #[test]
    fn tax_4pct_on_100_lamports() {
        // 4% of 100 lamports = 4 lamports
        assert_eq!(calculate_tax(100, 400), Some(4));
    }

    #[test]
    fn tax_4pct_rounds_down() {
        // 4% of 10 lamports = 0.4 -> 0 (floor)
        assert_eq!(calculate_tax(10, 400), Some(0));
    }

    #[test]
    fn tax_100pct_on_max() {
        // 100% tax on u64::MAX should return u64::MAX
        assert_eq!(calculate_tax(u64::MAX, 10000), Some(u64::MAX));
    }

    #[test]
    fn tax_invalid_bps_over_10000() {
        // tax_bps > 10000 is invalid -> None
        assert_eq!(calculate_tax(1_000_000_000, 10001), None);
    }

    #[test]
    fn tax_zero_input() {
        // Zero input -> zero tax
        assert_eq!(calculate_tax(0, 400), Some(0));
    }

    #[test]
    fn tax_zero_bps() {
        // 0% tax -> zero tax
        assert_eq!(calculate_tax(1_000_000_000, 0), Some(0));
    }

    #[test]
    fn tax_max_valid_bps() {
        // 100% tax (10000 bps) is valid edge case
        assert_eq!(calculate_tax(1000, 10000), Some(1000));
    }

    #[test]
    fn tax_1_bps_on_large_amount() {
        // 0.01% of 1 trillion lamports = 100 million lamports
        assert_eq!(calculate_tax(1_000_000_000_000, 1), Some(100_000_000));
    }

    // =========================================================================
    // Part B: split_distribution tests
    // =========================================================================

    #[test]
    fn split_100_lamports() {
        // 100 lamports: floor(100*7100/10000)=71, floor(100*2400/10000)=24, remainder=5
        assert_eq!(split_distribution(100), Some((71, 24, 5)));
    }

    #[test]
    fn split_1000_lamports() {
        // 1000 lamports: floor(1000*7100/10000)=710, floor(1000*2400/10000)=240, remainder=50
        assert_eq!(split_distribution(1000), Some((710, 240, 50)));
    }

    #[test]
    fn split_10_lamports_with_remainder() {
        // 10 lamports: floor(10*0.71)=7 staking, floor(10*0.24)=2 carnage, 10-7-2=1 treasury
        assert_eq!(split_distribution(10), Some((7, 2, 1)));
    }

    #[test]
    fn split_micro_tax_3_lamports() {
        // 3 lamports (< 4): all to staking per micro-tax rule
        assert_eq!(split_distribution(3), Some((3, 0, 0)));
    }

    #[test]
    fn split_micro_tax_1_lamport() {
        // 1 lamport (< 4): all to staking
        assert_eq!(split_distribution(1), Some((1, 0, 0)));
    }

    #[test]
    fn split_zero_tax() {
        // 0 lamports: all zeros
        assert_eq!(split_distribution(0), Some((0, 0, 0)));
    }

    #[test]
    fn split_max_u64() {
        // u64::MAX should not overflow with u128 intermediates
        let result = split_distribution(u64::MAX);
        assert!(result.is_some(), "u64::MAX should not overflow");

        let (staking, carnage, treasury) = result.unwrap();
        // Verify invariant: sum equals input
        assert_eq!(
            staking
                .checked_add(carnage)
                .and_then(|s| s.checked_add(treasury)),
            Some(u64::MAX),
            "Distribution sum must equal total_tax"
        );
    }

    #[test]
    fn split_4_lamports_boundary() {
        // 4 lamports (>= 4): normal split, not micro-tax
        // floor(4 * 7100 / 10000) = floor(2.84) = 2, floor(4 * 2400 / 10000) = floor(0.96) = 0, remainder = 2
        assert_eq!(split_distribution(4), Some((2, 0, 2)));
    }

    #[test]
    fn split_5_lamports() {
        // 5 lamports: floor(5*0.71)=3, floor(5*0.24)=1, 5-3-1=1
        assert_eq!(split_distribution(5), Some((3, 1, 1)));
    }

    // =========================================================================
    // Part C: Invariant tests
    // =========================================================================

    #[test]
    fn split_invariant_sum_equals_total() {
        // Test invariant across various values
        for total in [0, 1, 2, 3, 4, 5, 10, 99, 100, 101, 1000, 10000, 1_000_000] {
            let result = split_distribution(total);
            assert!(
                result.is_some(),
                "split_distribution({}) returned None",
                total
            );

            let (staking, carnage, treasury) = result.unwrap();
            let sum = staking + carnage + treasury;
            assert_eq!(
                sum, total,
                "Invariant violated for {}: {} + {} + {} = {} != {}",
                total, staking, carnage, treasury, sum, total
            );
        }
    }

    // =========================================================================
    // Part D: calculate_output_floor tests
    // =========================================================================

    #[test]
    fn floor_50pct_equal_reserves() {
        // Equal reserves (1000, 1000), amount_in=100, floor_bps=5000 (50%)
        // expected = 1000 * 100 / (1000 + 100) = 100_000 / 1100 = 90 (truncated)
        // floor = 90 * 5000 / 10_000 = 45
        let floor = calculate_output_floor(1000, 1000, 100, 5000);
        assert_eq!(floor, Some(45));
    }

    #[test]
    fn floor_zero_reserve_in() {
        // reserve_in = 0 -> returns 0 (empty pool, no floor enforceable)
        assert_eq!(calculate_output_floor(0, 1000, 100, 5000), Some(0));
    }

    #[test]
    fn floor_zero_reserve_out() {
        // reserve_out = 0 -> returns 0 (no output possible)
        assert_eq!(calculate_output_floor(1000, 0, 100, 5000), Some(0));
    }

    #[test]
    fn floor_zero_amount() {
        // amount_in = 0 -> returns 0 (nothing to swap)
        assert_eq!(calculate_output_floor(1000, 1000, 0, 5000), Some(0));
    }

    #[test]
    fn floor_100pct() {
        // floor_bps = 10000 (100%) -> floor equals expected output
        // expected = 1000 * 100 / (1000 + 100) = 90
        // floor = 90 * 10000 / 10000 = 90
        let floor = calculate_output_floor(1000, 1000, 100, 10000);
        assert_eq!(floor, Some(90));
    }

    #[test]
    fn floor_large_reserves_no_overflow() {
        // Test with u64::MAX-scale reserves to verify no overflow.
        // reserve_in = u64::MAX / 2, reserve_out = u64::MAX / 2, amount_in = u64::MAX / 4
        let reserve = u64::MAX / 2;
        let amount = u64::MAX / 4;
        let result = calculate_output_floor(reserve, reserve, amount, 5000);
        assert!(result.is_some(), "Large reserves should not overflow");

        // Verify floor is reasonable: expected ~ reserve * amount / (reserve + amount)
        // With equal reserves and amount = reserve/2:
        // expected = reserve * (reserve/2) / (reserve + reserve/2) = reserve/3
        // floor = (reserve/3) * 5000 / 10000 = reserve/6
        let floor = result.unwrap();
        let expected_approx = reserve / 6;
        // Allow 1 lamport tolerance for rounding
        assert!(
            floor >= expected_approx - 1 && floor <= expected_approx + 1,
            "Floor {} should be approximately {} (reserve/6)",
            floor,
            expected_approx
        );
    }

    #[test]
    fn floor_small_amount_rounds_down() {
        // Small amount that produces small expected output.
        // expected = 1_000_000 * 1 / (1_000_000 + 1) = 0 (truncated from ~0.999999)
        // floor = 0 * 5000 / 10_000 = 0
        let floor = calculate_output_floor(1_000_000, 1_000_000, 1, 5000);
        assert_eq!(floor, Some(0));
    }

    #[test]
    fn floor_realistic_sol_pool() {
        // Realistic scenario: 100 SOL and 1M tokens in pool, buying with 1 SOL
        // reserve_in = 100_000_000_000 (100 SOL in lamports)
        // reserve_out = 1_000_000_000_000 (1M tokens with 6 decimals)
        // amount_in = 1_000_000_000 (1 SOL)
        // expected = 1_000_000_000_000 * 1_000_000_000 / (100_000_000_000 + 1_000_000_000)
        //          = 1e21 / 101e9 = ~9_900_990_099
        // floor = ~9_900_990_099 * 5000 / 10_000 = ~4_950_495_049
        let floor = calculate_output_floor(100_000_000_000, 1_000_000_000_000, 1_000_000_000, 5000);
        assert!(floor.is_some());
        let f = floor.unwrap();
        // Approximately 4.95 billion (half of ~9.9 billion expected)
        assert!(f > 4_900_000_000, "Floor {} should be > 4.9B", f);
        assert!(f < 5_000_000_000, "Floor {} should be < 5.0B", f);
    }

    // =========================================================================
    // Part E: Proptest property-based tests
    // =========================================================================

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(10_000))]

            /// Property 1: calculate_tax never overflows for valid bps
            #[test]
            fn tax_never_overflows_valid_bps(
                amount in 0u64..=u64::MAX,
                bps in 0u16..=10000u16,
            ) {
                let result = calculate_tax(amount, bps);
                prop_assert!(result.is_some(), "calculate_tax({}, {}) returned None", amount, bps);
            }

            /// Property 2: calculate_tax returns None for invalid bps
            #[test]
            fn tax_none_for_invalid_bps(
                amount in 0u64..=u64::MAX,
                bps in 10001u16..=u16::MAX,
            ) {
                let result = calculate_tax(amount, bps);
                prop_assert!(result.is_none(), "calculate_tax({}, {}) should be None", amount, bps);
            }

            /// Property 3: tax is monotonic with respect to bps
            #[test]
            fn tax_monotonic_with_bps(
                amount in 1u64..=1_000_000_000_000u64,
                bps_low in 0u16..=9999u16,
                bps_delta in 1u16..=500u16,
            ) {
                let bps_high = bps_low.saturating_add(bps_delta).min(10000);

                if let (Some(tax_low), Some(tax_high)) = (
                    calculate_tax(amount, bps_low),
                    calculate_tax(amount, bps_high),
                ) {
                    prop_assert!(
                        tax_low <= tax_high,
                        "Monotonicity violated: tax({}, {})={} > tax({}, {})={}",
                        amount, bps_low, tax_low, amount, bps_high, tax_high
                    );
                }
            }

            /// Property 4: split_distribution sum equals input
            #[test]
            fn split_sum_equals_input(total in 0u64..=u64::MAX) {
                if let Some((staking, carnage, treasury)) = split_distribution(total) {
                    let sum = staking.saturating_add(carnage).saturating_add(treasury);
                    prop_assert_eq!(
                        sum, total,
                        "Invariant violated: {} + {} + {} = {} != {}",
                        staking, carnage, treasury, sum, total
                    );
                }
            }

            /// Property 5: staking >= 70% of total (unless micro-tax)
            #[test]
            fn staking_at_least_70_percent(total in 4u64..=u64::MAX) {
                // Only for non-micro-tax (>= 4 lamports)
                if let Some((staking, _, _)) = split_distribution(total) {
                    // staking should be at least floor(total * 70 / 100)
                    // Actually it's 71%, but we verify >= 70% as a safety bound
                    let min_staking = total / 100 * 70;
                    prop_assert!(
                        staking >= min_staking,
                        "Staking {} < 70% of {} (min {})",
                        staking, total, min_staking
                    );
                }
            }

            /// Property 6: micro-tax rule (< 4) sends all to staking
            #[test]
            fn micro_tax_all_to_staking(total in 0u64..4u64) {
                if let Some((staking, carnage, treasury)) = split_distribution(total) {
                    prop_assert_eq!(staking, total, "Micro-tax: staking should equal total");
                    prop_assert_eq!(carnage, 0, "Micro-tax: carnage should be 0");
                    prop_assert_eq!(treasury, 0, "Micro-tax: treasury should be 0");
                }
            }
        }
    }
}
