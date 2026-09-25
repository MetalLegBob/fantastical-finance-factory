use anchor_lang::prelude::*;

/// Whitelist authority configuration for the Transfer Hook program.
///
/// This PDA controls who can add whitelist entries. Once `authority` is set
/// to `None` via burn_authority instruction, the whitelist becomes immutable.
///
/// Seeds: [b"authority"]
/// Space: 8 (discriminator) + 33 (Option<Pubkey>) + 1 (bool) = 42 bytes
///
/// Spec reference: Transfer_Hook_Spec.md Section 6.1
#[account]
#[derive(InitSpace)]
pub struct WhitelistAuthority {
    /// Authority pubkey. None = authority has been burned (whitelist immutable).
    /// Option<Pubkey> serializes as 1 byte discriminant + 32 bytes pubkey = 33 bytes.
    pub authority: Option<Pubkey>,
    /// Whether this account has been initialized.
    pub initialized: bool,
}

impl WhitelistAuthority {
    /// PDA seed for the whitelist authority account.
    pub const SEED: &'static [u8] = b"authority";
}
