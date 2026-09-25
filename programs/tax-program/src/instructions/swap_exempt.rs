//! swap_exempt: Tax-exempt bidirectional swap for Carnage Fund.
//!
//! Supports both:
//! - BUY: SOL -> CRIME/FRAUD (direction = 0, AtoB)
//! - SELL: CRIME/FRAUD -> SOL (direction = 1, BtoA)
//!
//! The sell direction is required for Carnage's 2% sell-then-buy path
//! (Carnage_Fund_Spec Section 8.4).
//!
//! ARCHITECTURAL CONSTRAINT: This instruction adds CPI depth 1.
//! The full Carnage CPI chain is:
//!   Epoch::vrf_callback (entry) -> Tax::swap_exempt (depth 1)
//!   -> AMM::swap_sol_pool (depth 2) -> Token-2022::transfer_checked (depth 3)
//!   -> Transfer Hook::execute (depth 4 -- SOLANA LIMIT)
//!
//! DO NOT add any CPI calls to this instruction path beyond AMM.
//!
//! Source: Carnage_Fund_Spec.md Section 2, 8.4, 16.1

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    amm_program_id, epoch_program_id, CARNAGE_SIGNER_SEED, SWAP_AUTHORITY_SEED,
};
use crate::errors::TaxError;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Execute a tax-exempt swap for Carnage Fund (bidirectional).
///
/// Flow:
/// 1. Validate amount_in > 0
/// 2. Validate direction is 0 or 1
/// 3. Build and execute AMM CPI with swap_authority PDA signing
///
/// No tax calculation, no distribution, no slippage protection.
/// Carnage accepts market execution (Carnage_Fund_Spec.md Section 9.3).
///
/// # Arguments
/// * `amount_in` - Amount to swap (SOL for buy, token for sell)
/// * `direction` - 0 = buy (SOL->Token), 1 = sell (Token->SOL)
/// * `is_crime` - true = CRIME pool, false = FRAUD pool (for future logging)
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SwapExempt<'info>>,
    amount_in: u64,
    direction: u8,
    _is_crime: bool,
) -> Result<()> {
    // =========================================================================
    // 1. Validate input
    // =========================================================================
    require!(amount_in > 0, TaxError::InsufficientInput);

    // Validate direction: 0 = AtoB (buy), 1 = BtoA (sell)
    require!(direction <= 1, TaxError::InvalidPoolType);

    // =========================================================================
    // 2. Build and execute AMM CPI
    // =========================================================================

    // 2a. Build swap_authority PDA signer seeds
    let swap_authority_seeds: &[&[u8]] = &[SWAP_AUTHORITY_SEED, &[ctx.bumps.swap_authority]];

    // 2b. Build account metas for AMM swap_sol_pool instruction
    //     Order matches AMM's SwapSolPool struct (see swap_sol_pool.rs):
    //     swap_authority, pool, vault_a, vault_b, mint_a, mint_b,
    //     user_token_a, user_token_b, user, token_program_a, token_program_b
    //
    //     NOTE: For Carnage, user_token_* are Carnage's token accounts, and
    //     carnage_authority is passed as the "user" signer.
    let mut account_metas = vec![
        AccountMeta::new_readonly(ctx.accounts.swap_authority.key(), true), // signer
        AccountMeta::new(ctx.accounts.pool.key(), false),
        AccountMeta::new(ctx.accounts.pool_vault_a.key(), false),
        AccountMeta::new(ctx.accounts.pool_vault_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_b.key(), false),
        AccountMeta::new(ctx.accounts.user_token_a.key(), false),
        AccountMeta::new(ctx.accounts.user_token_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.carnage_authority.key(), true), // Carnage as "user"
        AccountMeta::new_readonly(ctx.accounts.token_program_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.token_program_b.key(), false),
    ];

    // 2c. Forward remaining_accounts for transfer hook support
    //     The AMM passes these to Token-2022 transfer_checked calls
    for account in ctx.remaining_accounts.iter() {
        if account.is_writable {
            account_metas.push(AccountMeta::new(account.key(), account.is_signer));
        } else {
            account_metas.push(AccountMeta::new_readonly(account.key(), account.is_signer));
        }
    }

    // 2d. Build instruction data for AMM swap_sol_pool
    //     Format: discriminator (8 bytes) + amount_in (8) + direction (1) + minimum_out (8)
    //
    //     Anchor discriminator = first 8 bytes of sha256("global:swap_sol_pool")
    //     Precomputed: sha256("global:swap_sol_pool")[0..8] = [0xde, 0x80, 0x1e, 0x7b, 0x55, 0x27, 0x91, 0x8a]
    //     Direction: passed through from caller (0=AtoB, 1=BtoA)
    //     minimum_out: 0 (no slippage protection per Carnage_Fund_Spec Section 9.3)
    const AMM_SWAP_SOL_POOL_DISCRIMINATOR: [u8; 8] =
        [0xde, 0x80, 0x1e, 0x7b, 0x55, 0x27, 0x91, 0x8a];
    const MINIMUM_OUTPUT: u64 = 0; // Carnage accepts market execution

    let mut ix_data = Vec::with_capacity(25);
    ix_data.extend_from_slice(&AMM_SWAP_SOL_POOL_DISCRIMINATOR);
    ix_data.extend_from_slice(&amount_in.to_le_bytes());
    ix_data.push(direction); // SwapDirection: 0=AtoB, 1=BtoA
    ix_data.extend_from_slice(&MINIMUM_OUTPUT.to_le_bytes());

    // 2e. Build the instruction
    let ix = Instruction {
        program_id: ctx.accounts.amm_program.key(),
        accounts: account_metas,
        data: ix_data,
    };

    // 2f. Build account infos for CPI (same order as account_metas, plus AMM program)
    let mut account_infos = vec![
        ctx.accounts.swap_authority.to_account_info(),
        ctx.accounts.pool.to_account_info(),
        ctx.accounts.pool_vault_a.to_account_info(),
        ctx.accounts.pool_vault_b.to_account_info(),
        ctx.accounts.mint_a.to_account_info(),
        ctx.accounts.mint_b.to_account_info(),
        ctx.accounts.user_token_a.to_account_info(),
        ctx.accounts.user_token_b.to_account_info(),
        ctx.accounts.carnage_authority.to_account_info(),
        ctx.accounts.token_program_a.to_account_info(),
        ctx.accounts.token_program_b.to_account_info(),
    ];

    // Forward remaining_accounts for transfer hook
    for account in ctx.remaining_accounts.iter() {
        account_infos.push(account.clone());
    }

    // Add AMM program account info (required for CPI)
    account_infos.push(ctx.accounts.amm_program.to_account_info());

    // 2g. Execute CPI with swap_authority PDA signature
    invoke_signed(&ix, &account_infos, &[swap_authority_seeds])?;

    // 2h. Emit ExemptSwap event for off-chain monitoring
    let clock = Clock::get()?;
    emit!(crate::events::ExemptSwap {
        authority: ctx.accounts.carnage_authority.key(),
        pool: ctx.accounts.pool.key(),
        amount_a: amount_in,
        direction,
        slot: clock.slot,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Account struct
// ---------------------------------------------------------------------------

/// Accounts for swap_exempt instruction (tax-exempt Carnage swaps).
///
/// This instruction is exclusively for Carnage Fund rebalancing.
/// The carnage_authority MUST be a PDA derived from Epoch Program with
/// seeds = [CARNAGE_SIGNER_SEED]. This is enforced via seeds::program constraint.
///
/// Source: Tax_Pool_Logic_Spec.md Section 13.3
#[derive(Accounts)]
pub struct SwapExempt<'info> {
    /// Carnage authority PDA from Epoch Program.
    ///
    /// CRITICAL SECURITY: seeds::program ensures this PDA is derived from Epoch Program.
    /// Only Epoch Program can produce a valid signer with these seeds.
    ///
    /// CROSS-PROGRAM DEPENDENCY:
    /// - Tax Program's CARNAGE_SIGNER_SEED must match Epoch Program's derivation
    /// - Tax Program's epoch_program_id() must match Epoch Program's declare_id!
    /// - If either mismatch, swap_exempt will reject all Carnage calls
    ///
    /// CHECK: PDA derived from Epoch Program seeds, validated by seeds::program constraint
    #[account(
        seeds = [CARNAGE_SIGNER_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub carnage_authority: Signer<'info>,

    /// Tax Program's swap_authority PDA - signs AMM CPI.
    /// Same derivation as swap_sol_buy/swap_sol_sell.
    ///
    /// CHECK: PDA derived from seeds, used as signer for CPI
    #[account(
        seeds = [SWAP_AUTHORITY_SEED],
        bump,
    )]
    pub swap_authority: AccountInfo<'info>,

    // === Pool State (AMM) ===
    /// AMM pool state - mutable for reserve updates
    /// CHECK: Validated in AMM CPI
    #[account(mut)]
    pub pool: AccountInfo<'info>,

    // === Pool Vaults ===
    /// Pool's WSOL vault (Token A)
    #[account(mut)]
    pub pool_vault_a: InterfaceAccount<'info, TokenAccount>,

    /// Pool's CRIME/FRAUD vault (Token B)
    #[account(mut)]
    pub pool_vault_b: InterfaceAccount<'info, TokenAccount>,

    // === Mints ===
    /// WSOL mint
    pub mint_a: InterfaceAccount<'info, Mint>,

    /// CRIME or FRAUD mint (Token-2022)
    pub mint_b: InterfaceAccount<'info, Mint>,

    // === Carnage Token Accounts ===
    /// Carnage's WSOL token account (or wrapping account)
    #[account(mut)]
    pub user_token_a: InterfaceAccount<'info, TokenAccount>,

    /// Carnage's CRIME/FRAUD token account
    #[account(mut)]
    pub user_token_b: InterfaceAccount<'info, TokenAccount>,

    // === Programs ===
    /// AMM Program for swap CPI
    /// CHECK: Address validated against known AMM program ID
    #[account(address = amm_program_id() @ TaxError::InvalidAmmProgram)]
    pub amm_program: AccountInfo<'info>,

    /// SPL Token program (for WSOL)
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token-2022 program (for CRIME/FRAUD)
    pub token_program_b: Interface<'info, TokenInterface>,

    /// System program (may be needed for hook accounts)
    pub system_program: Program<'info, System>,
}
