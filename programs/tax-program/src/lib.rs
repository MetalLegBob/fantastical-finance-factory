//! Dr Fraudsworth Tax Program
//!
//! Asymmetric taxation and atomic distribution for SOL pool swaps.
//! Routes swaps through the AMM with tax calculation and 3-way distribution:
//! - 71% to staking escrow
//! - 24% to carnage fund
//! - 5% to treasury (remainder)
//!
//! Source: Tax_Pool_Logic_Spec.md

#[cfg(all(feature = "devnet", feature = "localnet"))]
compile_error!("tax-program: features `devnet` and `localnet` are mutually exclusive — pick one");

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
pub static DRF_RELEASE_IDENTITY_TAX: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ|epoch=64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY|amm=AG7QjhAoUecWK8iDDtzB4RzQY2HA7RgHbPeZimJSGwU1|staking=3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC|treasury=HCD8YxaevtD5Ewq3pjbVC3PyCAzSgrhkYCQzFdFUMmCy|crime=H7QFHbQQxnuutXqaYD3NC3iJcGYZNAERxabnjEddEkby|fraud=7xgHHaaMT6iDcEGupW7ZTGquTBSaEfEaCbzj4PB1Xdg3";
#[cfg(all(
    not(feature = "no-entrypoint"),
    not(any(feature = "devnet", feature = "localnet"))
))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_TAX: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj|epoch=4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2|amm=5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR|staking=12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH|treasury=GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN|crime=cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc|fraud=FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5";

#[cfg(feature = "devnet")]
declare_id!("5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ");
#[cfg(not(feature = "devnet"))]
declare_id!("43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj");

#[program]
pub mod tax_program {
    use super::*;

    /// Execute a SOL -> CRIME/FRAUD swap with buy tax.
    ///
    /// Tax is deducted from SOL input before swap execution.
    /// Distribution: 71% staking, 24% carnage, 5% treasury.
    ///
    /// # Arguments
    /// * `amount_in` - Total SOL amount to spend (including tax)
    /// * `minimum_output` - Minimum tokens expected (slippage protection)
    /// * `is_crime` - true = CRIME pool, false = FRAUD pool
    pub fn swap_sol_buy<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapSolBuy<'info>>,
        amount_in: u64,
        minimum_output: u64,
        is_crime: bool,
    ) -> Result<()> {
        instructions::swap_sol_buy::handler(ctx, amount_in, minimum_output, is_crime)
    }

    /// Execute a CRIME/FRAUD -> SOL swap with sell tax.
    ///
    /// Tax is deducted from SOL output after swap execution.
    /// Distribution: 71% staking, 24% carnage, 5% treasury.
    ///
    /// # Arguments
    /// * `amount_in` - Token amount to sell
    /// * `minimum_output` - Minimum SOL to receive AFTER tax (slippage protection)
    /// * `is_crime` - true = CRIME pool, false = FRAUD pool
    pub fn swap_sol_sell<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapSolSell<'info>>,
        amount_in: u64,
        minimum_output: u64,
        is_crime: bool,
    ) -> Result<()> {
        instructions::swap_sol_sell::handler(ctx, amount_in, minimum_output, is_crime)
    }

    /// Initialize the WSOL intermediary account (one-time admin setup).
    /// Must be called before the first sell swap.
    /// Creates a WSOL token account at the intermediary PDA, owned by swap_authority.
    pub fn initialize_wsol_intermediary(ctx: Context<InitializeWsolIntermediary>) -> Result<()> {
        instructions::initialize_wsol_intermediary::handler(ctx)
    }

    /// Execute tax-exempt swap for Carnage Fund (bidirectional).
    ///
    /// Called by Epoch Program during Carnage rebalancing.
    /// No tax applied - only AMM LP fee (1%) applies.
    ///
    /// # Arguments
    /// * `amount_in` - Amount to swap (SOL for buy, token for sell)
    /// * `direction` - 0 = buy (SOL->Token), 1 = sell (Token->SOL)
    /// * `is_crime` - true = CRIME pool, false = FRAUD pool
    pub fn swap_exempt<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapExempt<'info>>,
        amount_in: u64,
        direction: u8,
        is_crime: bool,
    ) -> Result<()> {
        instructions::swap_exempt::handler(ctx, amount_in, direction, is_crime)
    }

    /// Execute a generic non-SOL quote -> CRIME/FRAUD taxed swap.
    ///
    /// Pool orientation and faction identity are derived from the pool. Tax is
    /// skimmed into the quote asset's sweep ATA, and faction transfer-hook
    /// accounts are forwarded through `remaining_accounts`.
    pub fn swap_spl_buy<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapSplBuy<'info>>,
        amount_in: u64,
        minimum_output: u64,
    ) -> Result<()> {
        instructions::swap_spl_buy::handler(ctx, amount_in, minimum_output)
    }

    /// Execute a generic CRIME/FRAUD -> non-SOL quote taxed swap.
    ///
    /// `minimum_output` is the quote amount received AFTER tax. The canonical
    /// sweep ATA is the AMM's quote-output account; tax rests there until a
    /// later withdraw/distribute flow, while one PDA-signed transfer forwards
    /// the net amount to the dedicated user quote destination.
    pub fn swap_spl_sell<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapSplSell<'info>>,
        amount_in: u64,
        minimum_output: u64,
    ) -> Result<()> {
        instructions::swap_spl_sell::handler(ctx, amount_in, minimum_output)
    }

    /// Execute a tax-free swap for an authorized ArbConfig wallet.
    ///
    /// Generic over all 22 faction pools. This bot lane is deliberately not
    /// pause-gated and needs no wallet whitelist entries: a whitelisted pool
    /// vault is the counterparty to every transfer-hook movement.
    pub fn swap_arb_wallet<'info>(
        ctx: Context<'_, '_, 'info, 'info, SwapArbWallet<'info>>,
        amount_in: u64,
        direction: u8,
        minimum_out: u64,
    ) -> Result<()> {
        instructions::swap_arb_wallet::handler(ctx, amount_in, direction, minimum_out)
    }

    /// Withdraw quote-asset tax from the canonical sweep ATA.
    ///
    /// Only an ArbConfig wallet may sign, and the destination owner must be a
    /// registered wallet or the ArbConfig authority. `amount == 0` means the
    /// full sweep balance; an already empty full sweep succeeds silently. This
    /// differs deliberately from `distribute_swept(0)`, which rejects.
    pub fn withdraw_sweep(ctx: Context<WithdrawSweep>, amount: u64) -> Result<()> {
        instructions::withdraw_sweep::handler(ctx, amount)
    }

    /// Distribute an exact nonzero SOL amount through the deployed 71/24/5 split.
    ///
    /// Only an ArbConfig wallet may sign. The caller's system account is the
    /// direct source, `lamports == 0` rejects, and no timing or pause law is
    /// enforced. Remainder and dust behavior come only from the frozen helper.
    pub fn distribute_swept(ctx: Context<DistributeSwept>, lamports: u64) -> Result<()> {
        instructions::distribute_swept::handler(ctx, lamports)
    }
}
