use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
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
    auditors: "Internal audits: SOS, BOK, VulnHunter (v1.3)",
    expiry: "2027-03-20"
}

#[cfg(all(not(feature = "no-entrypoint"), feature = "devnet"))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_AMM: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=AG7QjhAoUecWK8iDDtzB4RzQY2HA7RgHbPeZimJSGwU1|tax=5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ|epoch=64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY|crime=H7QFHbQQxnuutXqaYD3NC3iJcGYZNAERxabnjEddEkby|fraud=7xgHHaaMT6iDcEGupW7ZTGquTBSaEfEaCbzj4PB1Xdg3";
#[cfg(all(not(feature = "no-entrypoint"), not(feature = "devnet")))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_AMM: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR|tax=43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj|epoch=4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2|crime=cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc|fraud=FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5";

#[cfg(feature = "devnet")]
declare_id!("AG7QjhAoUecWK8iDDtzB4RzQY2HA7RgHbPeZimJSGwU1");
#[cfg(not(feature = "devnet"))]
declare_id!("5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR");

#[program]
pub mod amm {
    use super::*;

    /// Initialize the global AdminConfig PDA.
    ///
    /// Can only be called by the program's upgrade authority (deployer).
    /// The `admin` parameter sets who can create pools -- this can be a
    /// different key from the upgrade authority (e.g., a multisig).
    pub fn initialize_admin(ctx: Context<InitializeAdmin>, admin: Pubkey) -> Result<()> {
        instructions::initialize_admin::handler(ctx, admin)
    }

    /// Transfer the admin key to a new pubkey (e.g., Squads multisig vault).
    /// Only the current admin can call this. new_admin must not be Pubkey::default().
    pub fn transfer_admin(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
        instructions::transfer_admin::handler(ctx, new_admin)
    }

    /// Burns the admin key, permanently preventing new pool creation.
    /// Only the current admin can call this. Irreversible.
    ///
    /// # Accounts
    /// * `admin` - Current admin signer
    /// * `admin_config` - AdminConfig PDA (admin set to Pubkey::default())
    pub fn burn_admin(ctx: Context<BurnAdmin>) -> Result<()> {
        instructions::burn_admin::handler(ctx)
    }

    /// Initialize a new AMM pool with PDA-owned vaults and seed liquidity.
    ///
    /// Creates pool state PDA, vault token accounts (owned by pool PDA),
    /// and transfers initial liquidity atomically. Pool type is inferred
    /// from token programs, not caller-declared.
    ///
    /// # Arguments
    /// * `lp_fee_bps` - LP fee in basis points
    /// * `amount_a` - Initial seed amount for token A
    /// * `amount_b` - Initial seed amount for token B
    pub fn initialize_pool<'info>(
        ctx: Context<'_, '_, 'info, 'info, InitializePool<'info>>,
        lp_fee_bps: u16,
        amount_a: u64,
        amount_b: u64,
    ) -> Result<()> {
        instructions::initialize_pool::handler(ctx, lp_fee_bps, amount_a, amount_b)
    }

    /// Execute a swap in a SOL pool (CRIME/SOL or FRAUD/SOL).
    ///
    /// Routes between Token-2022 (CRIME/FRAUD) and SPL Token (WSOL) based on
    /// swap direction. LP fee is deducted before output calculation.
    ///
    /// # Arguments
    /// * `amount_in` - Input token amount (pre-fee)
    /// * `direction` - SwapDirection::AtoB or SwapDirection::BtoA
    /// * `minimum_amount_out` - Slippage protection floor
    pub fn swap_sol_pool<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapSolPool<'info>>,
        amount_in: u64,
        direction: SwapDirection,
        minimum_amount_out: u64,
    ) -> Result<()> {
        instructions::swap_sol_pool::handler(ctx, amount_in, direction, minimum_amount_out)
    }

    /// Add faction-side-exact protocol liquidity from an approved ArbConfig wallet.
    pub fn add_liquidity_double_sided<'info>(
        ctx: Context<'_, '_, 'info, 'info, AddLiquidityDoubleSided<'info>>,
        faction_amount: u64,
        max_quote: u64,
    ) -> Result<()> {
        instructions::add_liquidity_double_sided::handler(ctx, faction_amount, max_quote)
    }

    /// Remove an approved percentage of both reserves to allowlisted ATAs.
    pub fn remove_liquidity_double_sided<'info>(
        ctx: Context<'_, '_, 'info, 'info, RemoveLiquidityDoubleSided<'info>>,
        share_bps: u16,
    ) -> Result<()> {
        instructions::remove_liquidity_double_sided::handler(ctx, share_bps)
    }
}
