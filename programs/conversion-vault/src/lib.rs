//! Dr Fraudsworth Conversion Vault
//!
//! Fixed-rate 100:1 token conversions between CRIME/FRAUD and PROFIT.
//! Leaf-node program: calls only Token-2022, receives no CPIs.

// Phase 149 sos-005 hardening: features `devnet` and `localnet` are mutually
// exclusive. Enabling both at once would silently pick the localnet arm in
// extract_profit.rs's Accounts struct (cfg field-splitting resolution) and
// produce a broken devnet binary. This `compile_error!` makes the implicit
// invariant explicit — loud failure at build time.
#[cfg(all(feature = "devnet", feature = "localnet"))]
compile_error!(
    "conversion-vault: features `devnet` and `localnet` are mutually exclusive — pick one"
);

use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod helpers;
pub mod instructions;
pub mod state;

use instructions::*;

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Dr Fraudsworth's Finance Factory",
    project_url: "https://fraudsworth.fun",
    contacts: "email:drfraudsworth@gmail.com,twitter:@fraudsworth",
    policy: "https://fraudsworth.fun/docs/security/security-policy",
    preferred_languages: "en",
    auditors: "Internal audits: SOS #4, BOK, DB #3 (v1.5+)",
    expiry: "2027-03-20"
}

#[cfg(all(not(feature = "no-entrypoint"), feature = "devnet"))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_CONVERSION_VAULT: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=7XuvMcwDU5wWR8CSjWGsVwDJYVBnsWL7SXYDDdsSEVXK|crime=H7QFHbQQxnuutXqaYD3NC3iJcGYZNAERxabnjEddEkby|fraud=7xgHHaaMT6iDcEGupW7ZTGquTBSaEfEaCbzj4PB1Xdg3|profit=2t1K59GE2b4oXSRbZCyJo7VE3DXSJmtVBkE5kdrT3xLW|authority=HCD8YxaevtD5Ewq3pjbVC3PyCAzSgrhkYCQzFdFUMmCy|destination=4RNjDAUWK7xsQgqLmmVqV1cQ2B6T38n2Exs3hfSF6z2F|vault_profit=FnBL9thF3kPwCR45AmiW3bfw8sYNXhKNf5jcVHjA81s5";
#[cfg(all(
    not(feature = "no-entrypoint"),
    not(any(feature = "devnet", feature = "localnet"))
))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_CONVERSION_VAULT: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=5uawA6ehYTu69Ggvm3LSK84qFawPKxbWgfngwj15NRJ|crime=cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc|fraud=FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5|profit=pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR|authority=GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN|destination=2xWqYxbZb1a5khGq3Lx4EVm33zreFZKX4pfZZk6RLPeq|vault_profit=DBMaWgfUW8WBb8VVvqDFkrMpEkPkCPTcLpSpyzHAiwp3";

#[cfg(feature = "devnet")]
declare_id!("7XuvMcwDU5wWR8CSjWGsVwDJYVBnsWL7SXYDDdsSEVXK");
#[cfg(not(feature = "devnet"))]
declare_id!("5uawA6ehYTu69Ggvm3LSK84qFawPKxbWgfngwj15NRJ");

#[program]
pub mod conversion_vault {
    use super::*;

    /// One-shot vault initialization. Creates VaultConfig PDA and 3 token accounts.
    /// Any signer can call — no authority stored.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        instructions::initialize::handler(ctx)
    }

    /// Convert tokens at fixed 100:1 rate.
    /// Supports 4 paths: CRIME->PROFIT, FRAUD->PROFIT, PROFIT->CRIME, PROFIT->FRAUD.
    pub fn convert<'info>(
        ctx: Context<'_, '_, 'info, 'info, Convert<'info>>,
        amount_in: u64,
    ) -> Result<()> {
        instructions::convert::handler(ctx, amount_in)
    }

    /// Convert tokens at fixed 100:1 rate with on-chain balance reading and slippage protection.
    ///
    /// Three modes controlled by `amount_in` and `pre_balance`:
    /// - `amount_in > 0`: Exact mode — convert exactly `amount_in` tokens.
    /// - `amount_in == 0, pre_balance == 0`: Convert-all — convert entire balance.
    /// - `amount_in == 0, pre_balance > 0`: Delta mode — convert only the tokens
    ///   deposited since `pre_balance` (e.g. by a preceding AMM swap in an atomic
    ///   multi-hop route). User's pre-existing holdings are untouched.
    pub fn convert_v2<'info>(
        ctx: Context<'_, '_, 'info, 'info, Convert<'info>>,
        amount_in: u64,
        minimum_output: u64,
        pre_balance: u64,
    ) -> Result<()> {
        instructions::convert_v2::handler(ctx, amount_in, minimum_output, pre_balance)
    }

    /// Phase 148 — one-shot extraction of 4.5M PROFIT to the Squads vault PDA's PROFIT ATA.
    /// Spec: `.docs/v1.9/extract-profit-spec.md`. Callable ONLY by the Squads vault PDA;
    /// cannot be re-run (MigrationConfig PDA enforces one-shot via Anchor `init`).
    pub fn extract_profit<'info>(
        ctx: Context<'_, '_, 'info, 'info, ExtractProfit<'info>>,
    ) -> Result<()> {
        instructions::extract_profit::handler(ctx)
    }
}
