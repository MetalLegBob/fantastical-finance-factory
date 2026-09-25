use anchor_lang::prelude::*;

/// Global vault configuration PDA.
/// Seeds: ["vault_config"]
///
/// Minimal state — all conversion parameters are hardcoded constants.
/// No authority stored. No conversion rate stored.
/// Upgrade authority managed by Squads multisig on the program itself.
///
/// In localnet mode, mint addresses are stored in state (not hardcoded)
/// so integration tests with random mints can exercise the vault.
#[account]
pub struct VaultConfig {
    /// PDA bump seed for deterministic re-derivation.
    pub bump: u8,
    /// Localnet: CRIME mint address stored at init time.
    #[cfg(feature = "localnet")]
    pub crime_mint: Pubkey,
    /// Localnet: FRAUD mint address stored at init time.
    #[cfg(feature = "localnet")]
    pub fraud_mint: Pubkey,
    /// Localnet: PROFIT mint address stored at init time.
    #[cfg(feature = "localnet")]
    pub profit_mint: Pubkey,
    /// Localnet-only override: LiteSVM tests inject a runtime-generated keypair pubkey here so
    /// the `extract_profit` Accounts struct can pin `payer.address = vault_config.squads_vault_override`
    /// instead of the hardcoded mainnet Squads vault address. Mirrors crime_mint/fraud_mint/profit_mint
    /// localnet pattern. RESEARCH.md Q3.
    #[cfg(feature = "localnet")]
    pub squads_vault_override: Pubkey,
}

impl VaultConfig {
    /// Account size including 8-byte Anchor discriminator.
    #[cfg(not(feature = "localnet"))]
    pub const LEN: usize = 8 + 1; // discriminator + bump

    /// Localnet: 8 (disc) + 1 (bump) + 32*3 (mints) + 32 (squads_vault_override) = 137 bytes
    #[cfg(feature = "localnet")]
    pub const LEN: usize = 8 + 1 + 32 + 32 + 32 + 32;
}

// ---------------------------------------------------------------------------
// Phase 148 — MigrationConfig (one-shot extraction sentinel)
// ---------------------------------------------------------------------------

/// One-shot sentinel PDA whose mere existence blocks replay of `extract_profit`.
/// Created by Anchor `init` in `extract_profit`'s Accounts struct. Zero data fields:
/// only the 8-byte Anchor discriminator. Second invocation of `extract_profit` fails
/// at the System Program `create_account` CPI with `AccountAlreadyInUse` — deterministic
/// Solana runtime, not application logic. See spec § Section 7.
///
/// Phase 152 revert binary REMOVES the `extract_profit` IX selector; the PDA continues
/// to exist as inert storage. See spec § Section 9 + invariant I14.
#[account]
pub struct MigrationConfig {}

impl MigrationConfig {
    /// Account size: 8-byte Anchor discriminator only (zero data fields).
    pub const LEN: usize = 8;
}
