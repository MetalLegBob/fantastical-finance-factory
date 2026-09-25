use anchor_lang::prelude::*;

/// Global admin configuration for the AMM program.
///
/// This PDA is initialized once by the program's upgrade authority,
/// storing the admin pubkey that gates pool creation. The admin can
/// be a multisig address for operational security.
///
/// Seeds: [b"admin"]
#[account]
#[derive(InitSpace)]
pub struct AdminConfig {
    /// The admin pubkey authorized to create pools.
    /// Can be a multisig address -- not required to be the upgrade authority.
    pub admin: Pubkey,
    /// PDA bump seed for re-derivation in downstream instructions.
    pub bump: u8,
}
