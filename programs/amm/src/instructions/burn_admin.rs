use anchor_lang::prelude::*;

use crate::constants::ADMIN_SEED;
use crate::events::AdminBurned;
use crate::state::AdminConfig;

/// Burns the admin key by setting it to Pubkey::default().
/// After this call, initialize_pool's `has_one = admin` constraint
/// will always fail since no one can sign as Pubkey::default().
///
/// This is irreversible. Only the current admin can call this.
///
/// # Accounts
/// * `admin` - Current admin signer (must match admin_config.admin)
/// * `admin_config` - The AdminConfig PDA to modify
pub fn handler(ctx: Context<BurnAdmin>) -> Result<()> {
    let admin_config = &mut ctx.accounts.admin_config;
    let burned_by = admin_config.admin;
    let clock = Clock::get()?;

    // Permanently revoke admin privileges
    admin_config.admin = Pubkey::default();

    msg!(
        "Admin burned by {}. Pool creation permanently disabled.",
        burned_by
    );

    emit!(AdminBurned {
        burned_by,
        slot: clock.slot,
    });

    Ok(())
}

#[derive(Accounts)]
pub struct BurnAdmin<'info> {
    /// The current admin. Must match admin_config.admin.
    pub admin: Signer<'info>,

    /// The AdminConfig PDA. After this instruction, admin will be Pubkey::default().
    #[account(
        mut,
        seeds = [ADMIN_SEED],
        bump = admin_config.bump,
        has_one = admin @ crate::errors::AmmError::Unauthorized,
    )]
    pub admin_config: Account<'info, AdminConfig>,
}
