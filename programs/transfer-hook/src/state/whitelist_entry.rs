use anchor_lang::prelude::*;

/// A whitelisted address entry.
///
/// Whitelist uses existence-based PDA pattern: if this PDA exists for an address,
/// that address is whitelisted. PDA non-existence = not whitelisted.
///
/// Seeds: [b"whitelist", address.as_ref()]
/// Space: 8 (discriminator) + 32 (Pubkey) + 8 (i64) = 48 bytes
///
/// Spec reference: Transfer_Hook_Spec.md Section 5.3
#[account]
#[derive(InitSpace)]
pub struct WhitelistEntry {
    /// The whitelisted address (token account pubkey, not wallet).
    pub address: Pubkey,
    /// Timestamp when this entry was created (audit trail).
    pub created_at: i64,
}

impl WhitelistEntry {
    /// PDA seed prefix for whitelist entries.
    /// Full seeds: [SEED_PREFIX, address.as_ref()]
    pub const SEED_PREFIX: &'static [u8] = b"whitelist";
}
