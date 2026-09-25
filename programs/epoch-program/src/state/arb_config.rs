//! ArbConfig account and shared validation core.
//!
//! ArbConfig is the fixed-layout, cross-program root configuration for the
//! centralized-arbitrage MVP. Its account name and field order are part of the
//! serialized contract consumed by later phases.

use anchor_lang::prelude::*;

use crate::constants::{MAX_EPOCH_LENGTH_SLOTS, MAX_PAUSE_SLOTS, MIN_EPOCH_LENGTH_SLOTS};
use crate::errors::EpochError;

/// Global arbitrage and epoch-timing configuration.
///
/// Single PDA: seeds = `[b"arb_config"]`.
///
/// Source: Centralized-Arb MVP Spec §3.1, §14.1, and §14.6.
#[account]
#[repr(C)]
pub struct ArbConfig {
    /// Squads authority. Initialized from the verified ProgramData upgrade
    /// authority signer, never from an instruction argument.
    pub authority: Pubkey,

    /// Proposed successor for two-step authority transfer.
    /// `Pubkey::default()` means no transfer is pending.
    pub pending_authority: Pubkey,

    /// Primary protocol-owned arbitrage wallet.
    pub wallet_a: Pubkey,

    /// Standby protocol-owned arbitrage wallet.
    /// Equality with wallet_a is the sole allowed cross-role overlap.
    pub wallet_b: Pubkey,

    /// Cold authority allowed to set and clear the manual trading pause.
    pub pause_authority: Pubkey,

    /// Hot tripwire allowed to set, but never clear, the manual trading pause.
    pub pause_tripwire: Pubkey,

    /// Post-transition exclusivity pause in slots. Zero is legal.
    pub pause_slots: u64,

    /// Live-read epoch length in slots.
    pub epoch_length_slots: u64,

    /// PDA bump seed.
    pub bump: u8,

    /// Reserved padding for future fixed-layout evolution.
    pub reserved: [u8; 64],
}

impl ArbConfig {
    /// Serialized account data size, excluding Anchor's discriminator.
    pub const DATA_LEN: usize = 6 * 32 + 8 + 8 + 1 + 64;

    /// Total account size, including Anchor's 8-byte discriminator.
    pub const LEN: usize = 8 + Self::DATA_LEN;
}

// Freeze the Borsh data contract at 273 bytes (281 bytes on-chain).
const _: () = assert!(ArbConfig::DATA_LEN == 273);

/// Validate all numeric ArbConfig bounds.
///
/// This pure helper is the single source of truth shared by initialize,
/// update, and later proof harnesses.
pub fn validate_arb_config_bounds(pause_slots: u64, epoch_length_slots: u64) -> Result<()> {
    require!(
        pause_slots <= MAX_PAUSE_SLOTS,
        EpochError::PauseSlotsExceedsMax
    );
    require!(
        (MIN_EPOCH_LENGTH_SLOTS..=MAX_EPOCH_LENGTH_SLOTS).contains(&epoch_length_slots),
        EpochError::EpochLengthOutOfBounds
    );
    require!(
        pause_slots < epoch_length_slots,
        EpochError::PauseSlotsNotBelowEpochLength
    );

    Ok(())
}

/// Reject the default pubkey in every supplied externally controlled role.
///
/// Callers deliberately omit `pending_authority` when its zero value denotes
/// "no pending transfer".
pub fn validate_no_zero_pubkeys(keys: &[Pubkey]) -> Result<()> {
    for key in keys {
        require!(*key != Pubkey::default(), EpochError::ZeroPubkeyForbidden);
    }

    Ok(())
}

/// Validate pairwise separation of all active roles in the resulting config.
///
/// `wallet_a == wallet_b` is intentionally allowed so an emergency rotation
/// can point both wallet slots at the surviving key in one ceremony.
pub fn validate_role_overlap(
    authority: &Pubkey,
    wallet_a: &Pubkey,
    wallet_b: &Pubkey,
    pause_authority: &Pubkey,
    pause_tripwire: &Pubkey,
) -> Result<()> {
    require!(authority != wallet_a, EpochError::RoleOverlapForbidden);
    require!(authority != wallet_b, EpochError::RoleOverlapForbidden);
    require!(
        authority != pause_authority,
        EpochError::RoleOverlapForbidden
    );
    require!(
        authority != pause_tripwire,
        EpochError::RoleOverlapForbidden
    );
    require!(
        wallet_a != pause_authority,
        EpochError::RoleOverlapForbidden
    );
    require!(wallet_a != pause_tripwire, EpochError::RoleOverlapForbidden);
    require!(
        wallet_b != pause_authority,
        EpochError::RoleOverlapForbidden
    );
    require!(wallet_b != pause_tripwire, EpochError::RoleOverlapForbidden);
    require!(
        pause_authority != pause_tripwire,
        EpochError::RoleOverlapForbidden
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> Pubkey {
        Pubkey::new_from_array([byte; 32])
    }

    fn assert_error_name(result: Result<()>, expected: &str) {
        match result.expect_err("validation should reject this value") {
            anchor_lang::error::Error::AnchorError(error) => {
                assert_eq!(error.error_name, expected)
            }
            other => panic!("expected Anchor error {expected}, got {other:?}"),
        }
    }

    fn validate_roles(
        authority: Pubkey,
        wallet_a: Pubkey,
        wallet_b: Pubkey,
        pause_authority: Pubkey,
        pause_tripwire: Pubkey,
    ) -> Result<()> {
        validate_role_overlap(
            &authority,
            &wallet_a,
            &wallet_b,
            &pause_authority,
            &pause_tripwire,
        )
    }

    #[test]
    fn arb_config_layout_is_frozen() {
        assert_eq!(ArbConfig::DATA_LEN, 273);
        assert_eq!(ArbConfig::LEN, 281);
    }

    #[test]
    fn bounds_accept_pause_at_maximum() {
        validate_arb_config_bounds(300, 4_500).unwrap();
    }

    #[test]
    fn bounds_reject_pause_above_maximum() {
        assert_error_name(
            validate_arb_config_bounds(301, 4_500),
            "PauseSlotsExceedsMax",
        );
    }

    #[test]
    fn bounds_reject_epoch_below_minimum() {
        assert_error_name(validate_arb_config_bounds(0, 149), "EpochLengthOutOfBounds");
    }

    #[test]
    fn bounds_accept_epoch_at_minimum() {
        validate_arb_config_bounds(0, 150).unwrap();
    }

    #[test]
    fn bounds_accept_epoch_at_maximum() {
        validate_arb_config_bounds(300, 216_000).unwrap();
    }

    #[test]
    fn bounds_reject_epoch_above_maximum() {
        assert_error_name(
            validate_arb_config_bounds(300, 216_001),
            "EpochLengthOutOfBounds",
        );
    }

    #[test]
    fn bounds_reject_pause_equal_to_epoch_length() {
        assert_error_name(
            validate_arb_config_bounds(300, 300),
            "PauseSlotsNotBelowEpochLength",
        );
    }

    #[test]
    fn bounds_accept_pause_one_below_epoch_length() {
        validate_arb_config_bounds(149, 150).unwrap();
    }

    #[test]
    fn bounds_accept_zero_pause() {
        validate_arb_config_bounds(0, 4_500).unwrap();
    }

    #[test]
    fn zero_pubkey_validation_accepts_all_nonzero_roles() {
        validate_no_zero_pubkeys(&[key(1), key(2), key(3), key(4), key(5)]).unwrap();
    }

    #[test]
    fn zero_pubkey_validation_rejects_authority_position() {
        assert_error_name(
            validate_no_zero_pubkeys(&[Pubkey::default(), key(2), key(3), key(4), key(5)]),
            "ZeroPubkeyForbidden",
        );
    }

    #[test]
    fn zero_pubkey_validation_rejects_wallet_a_position() {
        assert_error_name(
            validate_no_zero_pubkeys(&[key(1), Pubkey::default(), key(3), key(4), key(5)]),
            "ZeroPubkeyForbidden",
        );
    }

    #[test]
    fn zero_pubkey_validation_rejects_wallet_b_position() {
        assert_error_name(
            validate_no_zero_pubkeys(&[key(1), key(2), Pubkey::default(), key(4), key(5)]),
            "ZeroPubkeyForbidden",
        );
    }

    #[test]
    fn zero_pubkey_validation_rejects_pause_authority_position() {
        assert_error_name(
            validate_no_zero_pubkeys(&[key(1), key(2), key(3), Pubkey::default(), key(5)]),
            "ZeroPubkeyForbidden",
        );
    }

    #[test]
    fn zero_pubkey_validation_rejects_pause_tripwire_position() {
        assert_error_name(
            validate_no_zero_pubkeys(&[key(1), key(2), key(3), key(4), Pubkey::default()]),
            "ZeroPubkeyForbidden",
        );
    }

    #[test]
    fn role_overlap_accepts_all_distinct_roles() {
        validate_roles(key(1), key(2), key(3), key(4), key(5)).unwrap();
    }

    #[test]
    fn role_overlap_accepts_wallet_a_equal_to_wallet_b() {
        validate_roles(key(1), key(2), key(2), key(4), key(5)).unwrap();
    }

    #[test]
    fn role_overlap_rejects_authority_equal_to_wallet_a() {
        assert_error_name(
            validate_roles(key(1), key(1), key(3), key(4), key(5)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_authority_equal_to_wallet_b() {
        assert_error_name(
            validate_roles(key(1), key(2), key(1), key(4), key(5)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_authority_equal_to_pause_authority() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(1), key(5)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_authority_equal_to_pause_tripwire() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(4), key(1)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_wallet_a_equal_to_pause_authority() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(2), key(5)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_wallet_a_equal_to_pause_tripwire() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(4), key(2)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_wallet_b_equal_to_pause_authority() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(3), key(5)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_wallet_b_equal_to_pause_tripwire() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(4), key(3)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn role_overlap_rejects_pause_authority_equal_to_pause_tripwire() {
        assert_error_name(
            validate_roles(key(1), key(2), key(3), key(4), key(4)),
            "RoleOverlapForbidden",
        );
    }
}
