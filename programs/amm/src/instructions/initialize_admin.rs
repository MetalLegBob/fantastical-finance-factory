use anchor_lang::prelude::*;

use crate::constants::ADMIN_SEED;
use crate::errors::AmmError;
use crate::state::AdminConfig;

/// Initializes the global AdminConfig PDA.
///
/// This instruction can only be called by the program's upgrade authority,
/// verified via the ProgramData account constraint. It stores the admin
/// pubkey that will gate pool creation -- the admin can be a different
/// key from the upgrade authority (e.g., a multisig).
///
/// This instruction can only be called once because `init` will fail if
/// the AdminConfig PDA already exists.
///
/// # Arguments
/// * `admin` - The pubkey to set as the AMM admin (can be multisig).
pub fn handler(ctx: Context<InitializeAdmin>, admin: Pubkey) -> Result<()> {
    // Defense in depth for fresh deployments. Existing mainnet state cannot
    // rerun this initializer, but a new singleton must never begin with the
    // permanently unsignable default key.
    require!(admin != Pubkey::default(), AmmError::InvalidAuthority);

    let admin_config = &mut ctx.accounts.admin_config;
    admin_config.admin = admin;
    admin_config.bump = ctx.bumps.admin_config;

    msg!("AdminConfig initialized. Admin: {}", admin);
    Ok(())
}

#[derive(Accounts)]
pub struct InitializeAdmin<'info> {
    /// The program's upgrade authority. Must sign to prove deployer identity.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// The AdminConfig PDA. Initialized once; stores the admin pubkey and bump.
    /// Seeds: [b"admin"]
    #[account(
        init,
        payer = authority,
        space = 8 + AdminConfig::INIT_SPACE,
        seeds = [ADMIN_SEED],
        bump
    )]
    pub admin_config: Account<'info, AdminConfig>,

    /// The AMM program itself -- used to look up its programdata address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::Amm>,

    /// The program's ProgramData account (created by the BPF loader on deploy).
    /// Constraint: its upgrade_authority_address must equal the signing authority.
    /// This is how we verify the caller is the deployer.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(authority.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    pub system_program: Program<'info, System>,
}
