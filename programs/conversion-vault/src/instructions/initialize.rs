use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    self, VAULT_CONFIG_SEED, VAULT_CRIME_SEED, VAULT_FRAUD_SEED, VAULT_PROFIT_SEED,
};
use crate::error::VaultError;
use crate::state::VaultConfig;

#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The program's upgrade authority. Must sign to prove deployer identity.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// VaultConfig singleton PDA.
    #[account(
        init,
        payer = payer,
        space = VaultConfig::LEN,
        seeds = [VAULT_CONFIG_SEED],
        bump,
    )]
    pub vault_config: Account<'info, VaultConfig>,

    /// Vault's CRIME token account — PDA-derived, authority = vault_config.
    #[account(
        init,
        payer = payer,
        token::mint = crime_mint,
        token::authority = vault_config,
        token::token_program = token_program,
        seeds = [VAULT_CRIME_SEED, vault_config.key().as_ref()],
        bump,
    )]
    pub vault_crime: InterfaceAccount<'info, TokenAccount>,

    /// Vault's FRAUD token account — PDA-derived, authority = vault_config.
    #[account(
        init,
        payer = payer,
        token::mint = fraud_mint,
        token::authority = vault_config,
        token::token_program = token_program,
        seeds = [VAULT_FRAUD_SEED, vault_config.key().as_ref()],
        bump,
    )]
    pub vault_fraud: InterfaceAccount<'info, TokenAccount>,

    /// Vault's PROFIT token account — PDA-derived, authority = vault_config.
    #[account(
        init,
        payer = payer,
        token::mint = profit_mint,
        token::authority = vault_config,
        token::token_program = token_program,
        seeds = [VAULT_PROFIT_SEED, vault_config.key().as_ref()],
        bump,
    )]
    pub vault_profit: InterfaceAccount<'info, TokenAccount>,

    /// CRIME mint (validated via constraint; bypassed in localnet).
    #[account(
        mint::token_program = token_program,
        constraint = cfg!(feature = "localnet") || crime_mint.key() == constants::crime_mint() @ VaultError::InvalidMintPair,
    )]
    pub crime_mint: InterfaceAccount<'info, Mint>,

    /// FRAUD mint (validated via constraint; bypassed in localnet).
    #[account(
        mint::token_program = token_program,
        constraint = cfg!(feature = "localnet") || fraud_mint.key() == constants::fraud_mint() @ VaultError::InvalidMintPair,
    )]
    pub fraud_mint: InterfaceAccount<'info, Mint>,

    /// PROFIT mint (validated via constraint; bypassed in localnet).
    #[account(
        mint::token_program = token_program,
        constraint = cfg!(feature = "localnet") || profit_mint.key() == constants::profit_mint() @ VaultError::InvalidMintPair,
    )]
    pub profit_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Interface<'info, TokenInterface>,

    /// The Conversion Vault program — used to look up its ProgramData address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::ConversionVault>,

    /// ProgramData account — upgrade_authority must match payer.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(payer.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<Initialize>) -> Result<()> {
    let vault_config = &mut ctx.accounts.vault_config;
    vault_config.bump = ctx.bumps.vault_config;

    // In localnet mode, store the actual mint addresses passed to initialize.
    // This allows integration tests with random mints to exercise the vault.
    #[cfg(feature = "localnet")]
    {
        vault_config.crime_mint = ctx.accounts.crime_mint.key();
        vault_config.fraud_mint = ctx.accounts.fraud_mint.key();
        vault_config.profit_mint = ctx.accounts.profit_mint.key();
    }

    Ok(())
}
