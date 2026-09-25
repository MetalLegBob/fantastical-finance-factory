//! Read-only mirror of Epoch Program's EpochState account.
//!
//! This struct MUST match the Epoch Program's EpochState layout exactly
//! for cross-program deserialization to work correctly.
//!
//! CRITICAL: The struct name "EpochState" must match exactly because
//! Anchor's discriminator is derived from sha256("account:EpochState").
//!
//! Source: Epoch_State_Machine_Spec.md Section 4.1

use anchor_lang::prelude::*;

/// Read-only mirror of Epoch Program's EpochState account.
/// Used for cross-program deserialization.
///
/// CRITICAL: Must match epoch-program's EpochState layout exactly,
/// including #[repr(C)] and reserved padding. The compile-time
/// assertion below enforces DATA_LEN parity.
#[account]
#[repr(C)]
pub struct EpochState {
    // Timing (8 + 4 + 8 = 20 bytes)
    pub genesis_slot: u64,
    pub current_epoch: u32,
    pub epoch_start_slot: u64,

    // Tax Configuration - what Tax Program reads (1 + 2 + 2 = 5 bytes)
    pub cheap_side: u8, // 0 = CRIME, 1 = FRAUD
    pub low_tax_bps: u16,
    pub high_tax_bps: u16,

    // Derived Tax Rates - cached for efficiency (2 * 4 = 8 bytes)
    pub crime_buy_tax_bps: u16,
    pub crime_sell_tax_bps: u16,
    pub fraud_buy_tax_bps: u16,
    pub fraud_sell_tax_bps: u16,

    // VRF State - Tax Program doesn't use but must match layout (8 + 1 + 1 + 32 = 42 bytes)
    pub vrf_request_slot: u64,
    pub vrf_pending: bool,
    pub taxes_confirmed: bool,
    pub pending_randomness_account: Pubkey,

    // Carnage State - Tax Program doesn't use but must match layout (1 + 1 + 1 + 8 + 8 + 4 = 23 bytes)
    pub carnage_pending: bool,
    pub carnage_target: u8,
    pub carnage_action: u8,
    pub carnage_deadline_slot: u64,
    pub carnage_lock_slot: u64,
    pub last_carnage_epoch: u32,

    // Reserved-byte carves — MUST match epoch-program's EpochState exactly.
    pub pause_end_slot: u64,
    pub trading_paused: bool,
    pub safety_version: u8,
    pub vrf_transition_start_slot: u64,
    pub pending_seed_slot: u64,
    pub vrf_attempts: u8,
    pub carnage_generation: u32,
    pub last_degraded_epoch: u32,
    pub last_vrf_fallback_reason: u8,
    pub reserved: [u8; 28],

    // Protocol (1 + 1 = 2 bytes)
    pub initialized: bool,
    pub bump: u8,
}
// Total remains 164 bytes; all new lifecycle fields consume existing padding.
// With 8-byte discriminator = 172 bytes

// DEF-08: Compile-time assertion -- mirror DATA_LEN must match epoch-program's EpochState.
// If this fails, the mirror struct has drifted from the source-of-truth.
const _: () = assert!(EpochState::DATA_LEN == 164);

impl EpochState {
    /// Data size without discriminator: 164 bytes.
    /// Must match epoch-program's EpochState::DATA_LEN exactly.
    pub const DATA_LEN: usize = 8
        + 4
        + 8
        + 1
        + 2
        + 2
        + 2
        + 2
        + 2
        + 2
        + 8
        + 1
        + 1
        + 32
        + 1
        + 1
        + 1
        + 8
        + 8
        + 4
        + 8
        + 1
        + 1
        + 8
        + 8
        + 1
        + 4
        + 4
        + 1
        + 28
        + 1
        + 1;

    /// Account size: 8 (discriminator) + 164 (data) = 172 bytes
    /// Source: Phase 47 added carnage_lock_slot (u64, +8 bytes).
    /// Phase 80 added reserved padding (+64 bytes).
    pub const LEN: usize = 8 + Self::DATA_LEN;

    /// Get the appropriate tax rate for a swap operation.
    ///
    /// # Arguments
    /// * `is_crime` - true for CRIME token, false for FRAUD token
    /// * `is_buy` - true for buy direction, false for sell direction
    ///
    /// # Returns
    /// Tax rate in basis points (e.g., 300 = 3%, 1400 = 14%)
    pub fn get_tax_bps(&self, is_crime: bool, is_buy: bool) -> u16 {
        match (is_crime, is_buy) {
            (true, true) => self.crime_buy_tax_bps,
            (true, false) => self.crime_sell_tax_bps,
            (false, true) => self.fraud_buy_tax_bps,
            (false, false) => self.fraud_sell_tax_bps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::AnchorDeserialize;

    #[test]
    fn test_reader_recovers_carved_fields_at_exact_offsets() {
        let mut data = vec![0u8; EpochState::DATA_LEN];

        data[0..8].copy_from_slice(&11u64.to_le_bytes());
        data[8..12].copy_from_slice(&7u32.to_le_bytes());
        data[12..20].copy_from_slice(&22u64.to_le_bytes());
        data[20] = 1;
        data[21..23].copy_from_slice(&300u16.to_le_bytes());
        data[23..25].copy_from_slice(&1_400u16.to_le_bytes());

        let expected_pause_end_slot = 0xABCD_EF01_2345_6789u64;
        data[98..106].copy_from_slice(&expected_pause_end_slot.to_le_bytes());
        data[106] = 1;
        data[162] = 1;
        data[163] = 247;

        let recovered = EpochState::deserialize(&mut data.as_slice())
            .expect("deserialize exact-offset EpochState bytes");

        assert_eq!(recovered.genesis_slot, 11);
        assert_eq!(recovered.current_epoch, 7);
        assert_eq!(recovered.epoch_start_slot, 22);
        assert_eq!(recovered.pause_end_slot, expected_pause_end_slot);
        assert!(recovered.trading_paused);
        assert!(recovered.initialized);
        assert_eq!(recovered.bump, 247);
    }
}
