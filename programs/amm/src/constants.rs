use anchor_lang::prelude::*;

/// Seed for the swap_authority PDA derived by Tax Program.
/// Both Tax Program and AMM must use identical seeds.
pub const SWAP_AUTHORITY_SEED: &[u8] = b"swap_authority";

/// Tax Program ID - the only program authorized to sign swap_authority.
/// This is hardcoded like SPL Token program IDs.
/// Production Tax Program ID (deployed in Phase 18-01).
#[cfg(feature = "devnet")]
pub const TAX_PROGRAM_ID: Pubkey = pubkey!("5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ");

#[cfg(not(feature = "devnet"))]
pub const TAX_PROGRAM_ID: Pubkey = pubkey!("43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj");

/// LP fee for SOL pools (CRIME/SOL, FRAUD/SOL) in basis points.
/// 100 bps = 1.0% fee per swap.
/// Source: AMM_Implementation.md Section 6
pub const SOL_POOL_FEE_BPS: u16 = 100;

/// Maximum LP fee in basis points.
/// 500 bps = 5% -- a reasonable upper bound to prevent admin misconfiguration.
/// Source: Phase 37 audit finding -- no upper bound on lp_fee_bps.
pub const MAX_LP_FEE_BPS: u16 = 500;

/// Basis points denominator (10,000 = 100%).
pub const BPS_DENOMINATOR: u128 = 10_000;

/// PDA seed for the global AdminConfig account.
pub const ADMIN_SEED: &[u8] = b"admin";

/// PDA seed prefix for pool state accounts.
/// Full seeds: [POOL_SEED, mint_a.as_ref(), mint_b.as_ref()]
pub const POOL_SEED: &[u8] = b"pool";

/// PDA seed prefix for pool vault token accounts.
/// Full seeds: [VAULT_SEED, pool.as_ref(), VAULT_A_SEED or VAULT_B_SEED]
pub const VAULT_SEED: &[u8] = b"vault";

/// PDA seed suffix for vault A.
pub const VAULT_A_SEED: &[u8] = b"a";

/// PDA seed suffix for vault B.
pub const VAULT_B_SEED: &[u8] = b"b";

/// Seed for the Epoch Program's singleton ArbConfig PDA.
pub const ARB_CONFIG_SEED: &[u8] = b"arb_config";

/// Epoch Program id — devnet arm, resolved from
/// `deployments/devnet.json` `.programs.epochProgram`. LiteSVM tests never
/// compile the devnet arm — they install fixtures at the canonical id.
#[cfg(feature = "devnet")]
pub fn epoch_program_id() -> Pubkey {
    pubkey!("64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY")
}

/// Epoch Program id — canonical mainnet and LiteSVM arm, resolved from
/// `deployments/mainnet.json` `.programs.epochProgram`.
#[cfg(not(feature = "devnet"))]
pub fn epoch_program_id() -> Pubkey {
    pubkey!("4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2")
}

/// CRIME mint — devnet arm, resolved from
/// `deployments/devnet.json` `.mints.crime`. LiteSVM tests never compile the
/// devnet arm — they install fixtures at the canonical id.
#[cfg(feature = "devnet")]
pub fn crime_mint() -> Pubkey {
    pubkey!("H7QFHbQQxnuutXqaYD3NC3iJcGYZNAERxabnjEddEkby")
}

/// CRIME mint — canonical mainnet and LiteSVM arm, resolved from
/// `deployments/mainnet.json` `.mints.crime`.
#[cfg(not(feature = "devnet"))]
pub fn crime_mint() -> Pubkey {
    pubkey!("cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc")
}

/// FRAUD mint — devnet arm, resolved from
/// `deployments/devnet.json` `.mints.fraud`. LiteSVM tests never compile the
/// devnet arm — they install fixtures at the canonical id.
#[cfg(feature = "devnet")]
pub fn fraud_mint() -> Pubkey {
    pubkey!("7xgHHaaMT6iDcEGupW7ZTGquTBSaEfEaCbzj4PB1Xdg3")
}

/// FRAUD mint — canonical mainnet and LiteSVM arm, resolved from
/// `deployments/mainnet.json` `.mints.fraud`.
#[cfg(not(feature = "devnet"))]
pub fn fraud_mint() -> Pubkey {
    pubkey!("FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "devnet")]
    #[test]
    fn epoch_program_id_pins_fresh_manifest() {
        let registry =
            include_str!("../../../.planning/phases/173-devnet-mirror-0b/173-IDENTITIES.json");
        let id = epoch_program_id().to_string();
        assert!(
            registry.contains(&format!("\"epochProgram\": \"{}\"", id)),
            "devnet epoch program id {id} not found at .programs.epochProgram"
        );
    }

    #[cfg(not(feature = "devnet"))]
    #[test]
    fn epoch_program_id_pins_mainnet_json() {
        let registry = include_str!("../../../deployments/mainnet.json");
        let id = epoch_program_id().to_string();
        assert!(
            registry.contains(&format!("\"epochProgram\": \"{}\"", id)),
            "canonical epoch program id {id} not found at .programs.epochProgram"
        );
    }

    #[cfg(feature = "devnet")]
    #[test]
    fn crime_mint_pins_fresh_manifest() {
        let registry =
            include_str!("../../../.planning/phases/173-devnet-mirror-0b/173-IDENTITIES.json");
        let id = crime_mint().to_string();
        assert!(
            registry.contains(&format!("\"crime\": \"{}\"", id)),
            "devnet CRIME mint {id} not found at .mints.crime"
        );
    }

    #[cfg(not(feature = "devnet"))]
    #[test]
    fn crime_mint_pins_mainnet_json() {
        let registry = include_str!("../../../deployments/mainnet.json");
        let id = crime_mint().to_string();
        assert!(
            registry.contains(&format!("\"crime\": \"{}\"", id)),
            "canonical CRIME mint {id} not found at .mints.crime"
        );
    }

    #[cfg(feature = "devnet")]
    #[test]
    fn fraud_mint_pins_fresh_manifest() {
        let registry =
            include_str!("../../../.planning/phases/173-devnet-mirror-0b/173-IDENTITIES.json");
        let id = fraud_mint().to_string();
        assert!(
            registry.contains(&format!("\"fraud\": \"{}\"", id)),
            "devnet FRAUD mint {id} not found at .mints.fraud"
        );
    }

    #[cfg(not(feature = "devnet"))]
    #[test]
    fn fraud_mint_pins_mainnet_json() {
        let registry = include_str!("../../../deployments/mainnet.json");
        let id = fraud_mint().to_string();
        assert!(
            registry.contains(&format!("\"fraud\": \"{}\"", id)),
            "canonical FRAUD mint {id} not found at .mints.fraud"
        );
    }
}
