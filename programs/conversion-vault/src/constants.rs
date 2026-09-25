use anchor_lang::prelude::*;
use std::str::FromStr;

/// Fixed conversion rate: 100 CRIME/FRAUD = 1 PROFIT.
/// Applied as integer division (CRIME->PROFIT) or multiplication (PROFIT->CRIME).
pub const CONVERSION_RATE: u64 = 100;

/// All project tokens use 6 decimals.
pub const TOKEN_DECIMALS: u8 = 6;

// ---------------------------------------------------------------------------
// PDA Seeds
// ---------------------------------------------------------------------------

pub const VAULT_CONFIG_SEED: &[u8] = b"vault_config";
pub const VAULT_CRIME_SEED: &[u8] = b"vault_crime";
pub const VAULT_FRAUD_SEED: &[u8] = b"vault_fraud";
pub const VAULT_PROFIT_SEED: &[u8] = b"vault_profit";

// ---------------------------------------------------------------------------
// Feature-Gated Mint Addresses
// ---------------------------------------------------------------------------

// Devnet arms resolve from the Phase-173 frozen identity graph instead of the
// superseded deployment registry. Mainnet and localnet arms remain unchanged.

#[cfg(feature = "devnet")]
pub fn crime_mint() -> Pubkey {
    Pubkey::from_str("H7QFHbQQxnuutXqaYD3NC3iJcGYZNAERxabnjEddEkby").unwrap()
}

#[cfg(feature = "localnet")]
pub fn crime_mint() -> Pubkey {
    // Localnet: placeholder, runtime-generated addresses used in tests.
    Pubkey::default()
}

#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub fn crime_mint() -> Pubkey {
    Pubkey::from_str("cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc").unwrap()
}

#[cfg(feature = "devnet")]
pub fn fraud_mint() -> Pubkey {
    Pubkey::from_str("7xgHHaaMT6iDcEGupW7ZTGquTBSaEfEaCbzj4PB1Xdg3").unwrap()
}

#[cfg(feature = "localnet")]
pub fn fraud_mint() -> Pubkey {
    Pubkey::default()
}

#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub fn fraud_mint() -> Pubkey {
    Pubkey::from_str("FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5").unwrap()
}

#[cfg(feature = "devnet")]
pub fn profit_mint() -> Pubkey {
    Pubkey::from_str("2t1K59GE2b4oXSRbZCyJo7VE3DXSJmtVBkE5kdrT3xLW").unwrap()
}

#[cfg(feature = "localnet")]
pub fn profit_mint() -> Pubkey {
    Pubkey::default()
}

#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub fn profit_mint() -> Pubkey {
    Pubkey::from_str("pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR").unwrap()
}

// ---------------------------------------------------------------------------
// Phase 148 — extract_profit constants (additive)
// ---------------------------------------------------------------------------

/// Fixed extraction amount: 4.5M PROFIT in raw u64 with PROFIT_DECIMALS=6.
/// NOT cfg-gated — same value on devnet + mainnet per CONTEXT lock (spec I1).
/// 4_500_000 * 10^6 = 4_500_000_000_000.
pub const EXTRACT_AMOUNT_RAW: u64 = 4_500_000_000_000;

/// PDA seed for the one-shot MigrationConfig sentinel.
/// NOT cfg-gated — PDA seed is the same on both clusters (spec §1).
pub const MIGRATION_CONFIG_SEED: &[u8] = b"migration_config";

// ---- Squads vault PDA (mainnet upgrade authority + extraction signer) ----

#[cfg(feature = "devnet")]
pub fn squads_vault() -> Pubkey {
    // Fresh devnet deployer is the pre-governance ceremony signer.
    Pubkey::from_str("HCD8YxaevtD5Ewq3pjbVC3PyCAzSgrhkYCQzFdFUMmCy").unwrap()
}

#[cfg(feature = "localnet")]
pub fn squads_vault() -> Pubkey {
    // Localnet: placeholder; LiteSVM tests use VaultConfig.squads_vault_override (runtime-generated keypair).
    Pubkey::default()
}

#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub fn squads_vault() -> Pubkey {
    // pre-state-snapshot.md § Section 4 — mainnet Squads multisig vault PDA (2-of-3 Ledger, 14400s timelock)
    Pubkey::from_str("GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN").unwrap()
}

// ---- Destination ATA (Squads vault PDA's PROFIT ATA) ----

#[cfg(feature = "devnet")]
pub fn destination_ata() -> Pubkey {
    // Frozen deployer PROFIT ATA from the Phase-173 topology.
    Pubkey::from_str("4RNjDAUWK7xsQgqLmmVqV1cQ2B6T38n2Exs3hfSF6z2F").unwrap()
}

#[cfg(feature = "localnet")]
pub fn destination_ata() -> Pubkey {
    Pubkey::default()
}

#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub fn destination_ata() -> Pubkey {
    // deployments/mainnet.json::whitelist.profitDestinationAta (canonical source).
    // Backstory: created + whitelisted in Phase 146; see pre-state-snapshot.md § Section 7.
    Pubkey::from_str("2xWqYxbZb1a5khGq3Lx4EVm33zreFZKX4pfZZk6RLPeq").unwrap()
}

// ---- Vault PROFIT ATA (Conversion Vault's PROFIT token account — extraction source) ----

#[cfg(feature = "devnet")]
pub fn vault_profit_ata() -> Pubkey {
    // Frozen conversion.profitVault PDA from the Phase-173 topology.
    Pubkey::from_str("FnBL9thF3kPwCR45AmiW3bfw8sYNXhKNf5jcVHjA81s5").unwrap()
}

#[cfg(feature = "localnet")]
pub fn vault_profit_ata() -> Pubkey {
    Pubkey::default()
}

#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub fn vault_profit_ata() -> Pubkey {
    // pre-state-snapshot.md § Section 2 — extraction source
    Pubkey::from_str("DBMaWgfUW8WBb8VVvqDFkrMpEkPkCPTcLpSpyzHAiwp3").unwrap()
}

// ---------------------------------------------------------------------------
// Phase 149 OBS-01 fix — `pub const` siblings of the 6 pubkey fns above.
// Anchor's `#[account(address = ...)]` constraint requires a const expression;
// `Pubkey::from_str(...).unwrap()` is non-const (Result + .unwrap() are runtime).
// `Pubkey::from_str_const(...)` IS const. These siblings exist so extract_profit.rs
// can pin its accounts via `address = constants::SQUADS_VAULT_PUBKEY` (one not-localnet
// arm, resolves correctly at compile time for devnet vs mainnet via cfg gating).
// Single source of truth for the literal value — mirrors the pub fn above.
// Localnet has no const versions because extract_profit's localnet arm uses
// runtime VaultConfig fields (squads_vault_override, profit_mint), never these consts.
// ---------------------------------------------------------------------------

#[cfg(feature = "devnet")]
pub const PROFIT_MINT_PUBKEY: Pubkey =
    Pubkey::from_str_const("2t1K59GE2b4oXSRbZCyJo7VE3DXSJmtVBkE5kdrT3xLW");
#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub const PROFIT_MINT_PUBKEY: Pubkey =
    Pubkey::from_str_const("pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR");

#[cfg(feature = "devnet")]
pub const SQUADS_VAULT_PUBKEY: Pubkey =
    Pubkey::from_str_const("HCD8YxaevtD5Ewq3pjbVC3PyCAzSgrhkYCQzFdFUMmCy");
#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub const SQUADS_VAULT_PUBKEY: Pubkey =
    Pubkey::from_str_const("GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN");

#[cfg(feature = "devnet")]
pub const DESTINATION_ATA_PUBKEY: Pubkey =
    Pubkey::from_str_const("4RNjDAUWK7xsQgqLmmVqV1cQ2B6T38n2Exs3hfSF6z2F");
#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub const DESTINATION_ATA_PUBKEY: Pubkey =
    Pubkey::from_str_const("2xWqYxbZb1a5khGq3Lx4EVm33zreFZKX4pfZZk6RLPeq");

#[cfg(feature = "devnet")]
pub const VAULT_PROFIT_ATA_PUBKEY: Pubkey =
    Pubkey::from_str_const("FnBL9thF3kPwCR45AmiW3bfw8sYNXhKNf5jcVHjA81s5");
#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub const VAULT_PROFIT_ATA_PUBKEY: Pubkey =
    Pubkey::from_str_const("DBMaWgfUW8WBb8VVvqDFkrMpEkPkCPTcLpSpyzHAiwp3");

// CRIME + FRAUD mint consts also exposed for future use (initialize.rs/convert.rs
// currently call the pub fn form; not migrated here to keep this fix minimal).
#[cfg(feature = "devnet")]
pub const CRIME_MINT_PUBKEY: Pubkey =
    Pubkey::from_str_const("H7QFHbQQxnuutXqaYD3NC3iJcGYZNAERxabnjEddEkby");
#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub const CRIME_MINT_PUBKEY: Pubkey =
    Pubkey::from_str_const("cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc");

#[cfg(feature = "devnet")]
pub const FRAUD_MINT_PUBKEY: Pubkey =
    Pubkey::from_str_const("7xgHHaaMT6iDcEGupW7ZTGquTBSaEfEaCbzj4PB1Xdg3");
#[cfg(not(any(feature = "devnet", feature = "localnet")))]
pub const FRAUD_MINT_PUBKEY: Pubkey =
    Pubkey::from_str_const("FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5");
