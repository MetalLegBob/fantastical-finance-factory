use anchor_lang::prelude::*;

use crate::errors::TransferHookError;
use crate::state::WhitelistAuthority;

/// Transfers the whitelist authority to a new pubkey (e.g., a Squads multisig vault).
///
/// Only the current authority can call this. The new_authority must not be
/// Pubkey::default() -- use burn_authority for permanent revocation.
///
/// If authority is already burned (None), this instruction fails with Unauthorized
/// because no signer can match a burned authority.
///
/// # Arguments
/// * `new_authority` - The pubkey to set as the new whitelist authority.
///
/// # Accounts
/// * `authority` - Current authority signer (must match whitelist_authority.authority)
/// * `whitelist_authority` - The WhitelistAuthority PDA to modify
pub fn handler(ctx: Context<TransferAuthority>, new_authority: Pubkey) -> Result<()> {
    // Prevent accidental burn -- burn_authority exists for that purpose
    require!(
        new_authority != Pubkey::default(),
        TransferHookError::Unauthorized
    );

    let auth = &mut ctx.accounts.whitelist_authority;

    // Verify signer is the current authority (mirrors burn_authority pattern)
    require!(
        auth.authority == Some(ctx.accounts.authority.key()),
        TransferHookError::Unauthorized
    );

    let old_authority = auth.authority.unwrap();
    auth.authority = Some(new_authority);

    msg!(
        "Authority transferred from {} to {}",
        old_authority,
        new_authority
    );

    Ok(())
}

/// Accounts for transfer_authority instruction.
///
/// No system_program needed -- we're modifying existing account, not creating.
#[derive(Accounts)]
pub struct TransferAuthority<'info> {
    /// Authority that will transfer control (must be current authority).
    pub authority: Signer<'info>,

    /// Whitelist authority PDA being transferred.
    #[account(
        mut,
        seeds = [WhitelistAuthority::SEED],
        bump
    )]
    pub whitelist_authority: Account<'info, WhitelistAuthority>,
}
