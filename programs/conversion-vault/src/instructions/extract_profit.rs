//! Phase 148 — extract_profit instruction.
//!
//! One-shot migration extraction: transfers exactly 4,500,000 PROFIT (raw u64 with 6 decimals)
//! from the Conversion Vault's PROFIT token account to the Squads vault PDA's PROFIT ATA.
//!
//! - Callable ONLY by the Squads vault PDA (EXTR-01)
//! - Transfers EXACTLY `EXTRACT_AMOUNT_RAW` (EXTR-02)
//! - Cannot be called twice — `init` on MigrationConfig PDA fails with AccountAlreadyInUse (EXTR-03)
//! - CRIME + FRAUD vaults structurally untouchable via triple constraint on vault_profit (EXTR-06)
//!
//! Spec: `.docs/v1.9/extract-profit-spec.md` § Sections 2 + 3. Phase 149 SOS audit verifies
//! code matches spec grep-by-grep using the 12 `PASS-149:` tags in spec § Section 6.
//!
//! Implementation notes:
//! - The `address = ...` constraints below use `Pubkey::from_str_const` literals per spec § Section 1
//!   (Anchor requires const expression in `#[account(address=...)]`). The constant-fn form
//!   `squads_vault() / destination_ata() / vault_profit_ata()` from `constants.rs` is used inside
//!   `fn -> Pubkey` bodies for runtime callsites, not here.
//! - `#[cfg(feature = "localnet")]` arm uses `vault_config.squads_vault_override` (a runtime-
//!   generated keypair stored in VaultConfig at test setup) instead of the mainnet literal,
//!   so LiteSVM tests can mock the Squads vault signer without deploying Squads.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::*;
use crate::helpers;
use crate::state::{MigrationConfig, VaultConfig};

pub fn handler<'info>(ctx: Context<'_, '_, 'info, 'info, ExtractProfit<'info>>) -> Result<()> {
    // Step 1 (implicit): Anchor `init` on `migration_config` already ran BEFORE this handler body.
    //   System Program create_account aborts with AccountAlreadyInUse on second invocation.
    //   Replay defense is runtime, not application logic. See spec § Section 7.

    // Step 2: CPI to Token-2022 transfer_checked via the canonical hook_helper.
    //   Reuse `helpers::hook_helper::transfer_t22_checked` AS-IS (spec I10).
    //   NOT Anchor's `token_interface::transfer_checked` — that does NOT forward
    //   remaining_accounts through the nested CPI chain (see hook_helper.rs:11-22).
    let vault_bump = ctx.accounts.vault_config.bump;
    let signer_seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, &[vault_bump]]];

    helpers::hook_helper::transfer_t22_checked(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.vault_profit.to_account_info(),
        &ctx.accounts.profit_mint.to_account_info(),
        &ctx.accounts.destination_ata.to_account_info(),
        &ctx.accounts.vault_config.to_account_info(),
        EXTRACT_AMOUNT_RAW,
        TOKEN_DECIMALS,
        signer_seeds,
        ctx.remaining_accounts,
    )?;

    // Step 3: emit single on-chain log for observability.
    msg!("extract_profit executed: amount={}", EXTRACT_AMOUNT_RAW);

    Ok(())
}

#[derive(Accounts)]
pub struct ExtractProfit<'info> {
    /// Singleton vault state PDA (signer for the transfer CPI via vault_config bump).
    #[account(
        seeds = [VAULT_CONFIG_SEED],
        bump = vault_config.bump,
    )]
    pub vault_config: Account<'info, VaultConfig>,

    /// Conversion Vault's PROFIT token account — extraction SOURCE.
    /// TRIPLE CONSTRAINT (defense-in-depth, spec I5):
    ///   - `token::mint = profit_mint` rejects CRIME/FRAUD vault substitution at framework layer
    ///   - `token::authority = vault_config` pins ownership to the vault PDA
    ///   - `address = ...` removes any remaining substitution ambiguity (cluster literal pinned)
    /// Phase 149 OBS-01 fix: `address` now sourced from cfg-gated `constants::VAULT_PROFIT_ATA_PUBKEY`
    /// so one arm resolves correctly for BOTH devnet and mainnet builds.
    #[cfg(not(feature = "localnet"))]
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = vault_config,
        address = VAULT_PROFIT_ATA_PUBKEY,
    )]
    pub vault_profit: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Localnet: drop the address pin (mainnet literal not applicable to runtime-generated ATAs)
    /// but KEEP mint + authority constraints so the triple-constraint logic is still exercised.
    // I5 LOCALNET EXCEPTION: address pin dropped because vault_profit ATA is runtime-generated
    // (no compile-time literal exists for a per-test ATA spawned by LiteSVM setup). The remaining
    // two of three I5 constraints (`token::mint` + `token::authority`) are still active and
    // enforced by Anchor's framework layer BEFORE the handler body runs — cross-mint substitution
    // (THREAT-03) still fails at ConstraintTokenMint, and authority forgery (THREAT-08) still
    // fails at ConstraintTokenOwner. Full three-constraint check (address pin) reverts on the
    // mainnet arm above. See RESEARCH.md Q3 for full rationale (LiteSVM cannot pin a runtime-
    // generated pubkey at compile time; same pattern Anchor itself uses in localnet ATA tests).
    #[cfg(feature = "localnet")]
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = vault_config,
    )]
    pub vault_profit: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Squads vault PDA's PROFIT ATA — extraction DESTINATION.
    /// `token::mint = profit_mint` + `token::authority = SQUADS_VAULT_PUBKEY` + `address = DESTINATION_ATA_PUBKEY`
    /// Phase 149 OBS-01 fix: literals migrated to cfg-gated consts; one arm covers devnet + mainnet.
    #[cfg(not(feature = "localnet"))]
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = SQUADS_VAULT_PUBKEY,
        address = DESTINATION_ATA_PUBKEY,
    )]
    pub destination_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Localnet: drop the address pin (runtime-generated destination) but KEEP mint
    /// constraint + dynamic-authority constraint sourced from VaultConfig.squads_vault_override.
    #[cfg(feature = "localnet")]
    #[account(
        mut,
        token::mint = profit_mint,
        token::authority = vault_config.squads_vault_override,
    )]
    pub destination_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// One-shot sentinel. `init` fails with AccountAlreadyInUse on second invocation.
    /// Spec I7 — MUST be `init`, NEVER `init_if_needed` (replay defense).
    #[account(
        init,
        payer = payer,
        seeds = [MIGRATION_CONFIG_SEED],
        bump,
        space = MigrationConfig::LEN,
    )]
    pub migration_config: Account<'info, MigrationConfig>,

    /// Must be the Squads vault PDA — the only entity authorized to extract (spec I6).
    /// Phase 149 OBS-01 fix: pinned via cfg-gated `SQUADS_VAULT_PUBKEY` const
    /// (devnet: `HM6PJAr...`, mainnet: `GDY4Qu3...`). Localnet: runtime override.
    #[cfg(not(feature = "localnet"))]
    #[account(
        mut,
        address = SQUADS_VAULT_PUBKEY,
    )]
    pub payer: Signer<'info>,

    #[cfg(feature = "localnet")]
    #[account(
        mut,
        address = vault_config.squads_vault_override,
    )]
    pub payer: Signer<'info>,

    /// PROFIT mint (pinned to address for explicit defense-in-depth).
    /// Phase 149 OBS-01 + OBS-10 fix: spec I2 narrative said "mainnet AND devnet share
    /// the same PROFIT mint pubkey" — UNTRUE in reality. Devnet has its own PROFIT mint
    /// (`Eaipvk74...`), distinct from mainnet vanity (`pRoFiTj36...`). cfg-gated const
    /// now correctly resolves per cluster.
    #[cfg(not(feature = "localnet"))]
    #[account(
        address = PROFIT_MINT_PUBKEY,
    )]
    pub profit_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Localnet: drop the address pin; mint is runtime-generated stored in VaultConfig.profit_mint.
    #[cfg(feature = "localnet")]
    #[account(
        address = vault_config.profit_mint,
    )]
    pub profit_mint: Box<InterfaceAccount<'info, Mint>>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,

    /// CHECK: forwarded as a hook account for Token-2022 transfer_checked.
    pub transfer_hook_program: UncheckedAccount<'info>,
    /// CHECK: ExtraAccountMetaList PDA for the PROFIT mint's transfer hook.
    pub extra_account_meta_list: UncheckedAccount<'info>,
    /// CHECK: WhitelistEntry PDA for vault_profit (source).
    pub whitelist_source: UncheckedAccount<'info>,
    /// CHECK: WhitelistEntry PDA for destination_ata (destination).
    pub whitelist_dest: UncheckedAccount<'info>,
}
