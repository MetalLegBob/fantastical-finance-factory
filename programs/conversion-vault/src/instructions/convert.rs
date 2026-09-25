use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{self, CONVERSION_RATE, TOKEN_DECIMALS, VAULT_CONFIG_SEED};
use crate::error::VaultError;
use crate::helpers;
use crate::state::VaultConfig;

#[derive(Accounts)]
pub struct Convert<'info> {
    /// User performing the conversion.
    pub user: Signer<'info>,

    /// VaultConfig PDA — needed for vault token account authority.
    #[account(
        seeds = [VAULT_CONFIG_SEED],
        bump = vault_config.bump,
    )]
    pub vault_config: Account<'info, VaultConfig>,

    /// User's input token account (source — user sends tokens here).
    #[account(mut)]
    pub user_input_account: InterfaceAccount<'info, TokenAccount>,

    /// User's output token account (destination — user receives tokens here).
    #[account(mut)]
    pub user_output_account: InterfaceAccount<'info, TokenAccount>,

    /// Input mint (CRIME, FRAUD, or PROFIT).
    pub input_mint: InterfaceAccount<'info, Mint>,

    /// Output mint (CRIME, FRAUD, or PROFIT).
    pub output_mint: InterfaceAccount<'info, Mint>,

    /// Vault's input token account (receives user's input tokens).
    /// Validated: correct mint + correct PDA authority.
    #[account(
        mut,
        token::authority = vault_config,
        token::mint = input_mint,
    )]
    pub vault_input: InterfaceAccount<'info, TokenAccount>,

    /// Vault's output token account (sends converted tokens to user).
    /// Validated: correct mint + correct PDA authority.
    #[account(
        mut,
        token::authority = vault_config,
        token::mint = output_mint,
    )]
    pub vault_output: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

/// Compute the output amount for a given conversion using hardcoded mint addresses.
///
/// Delegates to `compute_output_with_mints` with addresses from constants.
/// Used in production (devnet/mainnet) where mints are known at compile time.
pub fn compute_output(input_mint: &Pubkey, output_mint: &Pubkey, amount_in: u64) -> Result<u64> {
    compute_output_with_mints(
        input_mint,
        output_mint,
        amount_in,
        &constants::crime_mint(),
        &constants::fraud_mint(),
        &constants::profit_mint(),
    )
}

/// Core conversion math with explicit mint addresses.
///
/// Extracted so both production (hardcoded mints) and localnet (stored mints)
/// can share the same logic.
///
/// # Conversion rules
/// - CRIME/FRAUD -> PROFIT: divide by 100 (integer division, remainder lost)
/// - PROFIT -> CRIME/FRAUD: multiply by 100
///
/// # Errors
/// - `ZeroAmount` if `amount_in` is 0
/// - `SameMint` if input and output are the same mint
/// - `OutputTooSmall` if CRIME/FRAUD->PROFIT division yields 0
/// - `InvalidMintPair` if the pair is not CRIME<->PROFIT or FRAUD<->PROFIT
/// - `MathOverflow` if PROFIT->CRIME/FRAUD multiplication overflows u64
pub fn compute_output_with_mints(
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    amount_in: u64,
    crime: &Pubkey,
    fraud: &Pubkey,
    profit: &Pubkey,
) -> Result<u64> {
    require!(amount_in > 0, VaultError::ZeroAmount);
    require!(input_mint != output_mint, VaultError::SameMint);

    if (*input_mint == *crime || *input_mint == *fraud) && *output_mint == *profit {
        // CRIME/FRAUD -> PROFIT: divide by 100
        let out = amount_in / CONVERSION_RATE;
        require!(out > 0, VaultError::OutputTooSmall);
        Ok(out)
    } else if *input_mint == *profit && (*output_mint == *crime || *output_mint == *fraud) {
        // PROFIT -> CRIME/FRAUD: multiply by 100
        amount_in
            .checked_mul(CONVERSION_RATE)
            .ok_or_else(|| error!(VaultError::MathOverflow))
    } else {
        err!(VaultError::InvalidMintPair)
    }
}

pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, Convert<'info>>,
    amount_in: u64,
) -> Result<()> {
    // --- 1. Compute output (validates amount, mint pair, dust) ---
    let input_key = ctx.accounts.input_mint.key();
    let output_key = ctx.accounts.output_mint.key();

    // In localnet mode, read mint addresses from VaultConfig state (set at init).
    // In production, use hardcoded constants.
    #[cfg(feature = "localnet")]
    let amount_out = {
        let vc = &ctx.accounts.vault_config;
        compute_output_with_mints(
            &input_key,
            &output_key,
            amount_in,
            &vc.crime_mint,
            &vc.fraud_mint,
            &vc.profit_mint,
        )?
    };
    #[cfg(not(feature = "localnet"))]
    let amount_out = compute_output(&input_key, &output_key, amount_in)?;

    // --- 2. Split remaining_accounts for hook resolution ---
    // Layout: [input_hooks (4), output_hooks (4)]
    // D6: split at midpoint, same as AMM swap_profit_pool pattern
    let remaining = ctx.remaining_accounts;
    let mid = remaining.len() / 2;
    let (input_hooks, output_hooks) = remaining.split_at(mid);

    // --- 3. Transfer input: user -> vault (user-signed) ---
    helpers::hook_helper::transfer_t22_checked(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.user_input_account.to_account_info(),
        &ctx.accounts.input_mint.to_account_info(),
        &ctx.accounts.vault_input.to_account_info(),
        &ctx.accounts.user.to_account_info(),
        amount_in,
        TOKEN_DECIMALS,
        &[], // user is signer, not PDA
        input_hooks,
    )?;

    // --- 4. Transfer output: vault -> user (PDA-signed) ---
    let vault_bump = ctx.accounts.vault_config.bump;
    let signer_seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, &[vault_bump]]];

    helpers::hook_helper::transfer_t22_checked(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.vault_output.to_account_info(),
        &ctx.accounts.output_mint.to_account_info(),
        &ctx.accounts.user_output_account.to_account_info(),
        &ctx.accounts.vault_config.to_account_info(),
        amount_out,
        TOKEN_DECIMALS,
        signer_seeds,
        output_hooks,
    )?;

    Ok(())
}
