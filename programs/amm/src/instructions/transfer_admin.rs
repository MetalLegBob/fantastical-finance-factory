use anchor_lang::prelude::*;

use crate::constants::ADMIN_SEED;
use crate::errors::AmmError;
use crate::state::AdminConfig;

/// Transfers the admin key to a new pubkey (e.g., a Squads multisig vault).
///
/// Only the current admin can call this. The new_admin must not be
/// Pubkey::default() -- use burn_admin for permanent revocation.
///
/// This is the mechanism for moving admin authority to a Squads
/// timelocked multisig as part of the governance migration.
///
/// # Arguments
/// * `new_admin` - The pubkey to set as the new AMM admin.
///
/// # Accounts
/// * `admin` - Current admin signer (must match admin_config.admin)
/// * `admin_config` - The AdminConfig PDA to modify
pub fn handler(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
    // Prevent accidental burn -- burn_admin exists for that purpose
    require!(new_admin != Pubkey::default(), AmmError::InvalidAuthority);

    let admin_config = &mut ctx.accounts.admin_config;
    let old_admin = admin_config.admin;

    admin_config.admin = new_admin;

    msg!("Admin transferred from {} to {}", old_admin, new_admin);

    Ok(())
}

#[derive(Accounts)]
pub struct TransferAdmin<'info> {
    /// The current admin. Must match admin_config.admin.
    pub admin: Signer<'info>,

    /// The AdminConfig PDA. After this instruction, admin will be new_admin.
    #[account(
        mut,
        seeds = [ADMIN_SEED],
        bump = admin_config.bump,
        has_one = admin @ AmmError::Unauthorized,
    )]
    pub admin_config: Account<'info, AdminConfig>,
}
