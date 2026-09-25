//! Read-only mirror of the Epoch Program's ArbConfig account.
//!
//! This struct MUST match `programs/epoch-program/src/state/arb_config.rs`
//! exactly. Its name MUST remain `ArbConfig` because Anchor derives the
//! discriminator from sha256("account:ArbConfig"). The AMM only deserializes
//! this mirror and NEVER writes an Epoch Program ArbConfig account.

use anchor_lang::prelude::*;

/// Read-only cross-program mirror of the frozen ArbConfig layout.
#[account]
#[repr(C)]
pub struct ArbConfig {
    pub authority: Pubkey,
    pub pending_authority: Pubkey,
    pub wallet_a: Pubkey,
    pub wallet_b: Pubkey,
    pub pause_authority: Pubkey,
    pub pause_tripwire: Pubkey,
    pub pause_slots: u64,
    pub epoch_length_slots: u64,
    pub bump: u8,
    pub reserved: [u8; 64],
}

impl ArbConfig {
    /// Serialized account data size, excluding Anchor's discriminator.
    pub const DATA_LEN: usize = 6 * 32 + 8 + 8 + 1 + 64;
}

// Freeze the cross-program data contract at 273 bytes (281 bytes on-chain).
const _: () = assert!(ArbConfig::DATA_LEN == 273);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arb_config_discriminator_matches_frozen_bytes() {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(b"account:ArbConfig");
        let hash = hasher.finalize();
        assert_eq!(hash[..8], [135, 81, 18, 100, 131, 57, 107, 219]);
    }

    #[test]
    fn arb_config_layout_is_frozen() {
        assert_eq!(ArbConfig::DATA_LEN, 273);
        assert_eq!(8 + ArbConfig::DATA_LEN, 281);
    }
}
