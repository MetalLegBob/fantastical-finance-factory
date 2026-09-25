use crate::errors::TransferHookError;
use crate::events::AddressWhitelisted;
use crate::state::{WhitelistAuthority, WhitelistEntry};
use anchor_lang::prelude::*;

/// Add an address to the whitelist.
///
/// Creates WhitelistEntry PDA for the given address. Only callable by
/// the whitelist authority while authority is not burned.
///
/// Spec reference: Transfer_Hook_Spec.md Section 7.2
pub fn handler(ctx: Context<AddWhitelistEntry>) -> Result<()> {
    let auth = &ctx.accounts.whitelist_authority;
    let address = ctx.accounts.address_to_whitelist.key();

    // Authority check (constraint already ensures authority.is_some())
    require!(
        auth.authority == Some(ctx.accounts.authority.key()),
        TransferHookError::Unauthorized
    );

    // Address validation: reject system program and null pubkey
    require!(
        address != Pubkey::default() && address != anchor_lang::system_program::ID,
        TransferHookError::InvalidWhitelistPDA
    );

    // Populate entry
    let entry = &mut ctx.accounts.whitelist_entry;
    entry.address = address;
    entry.created_at = Clock::get()?.unix_timestamp;

    // Emit event per spec Section 7.2
    emit!(AddressWhitelisted {
        address: entry.address,
        added_by: ctx.accounts.authority.key(),
        timestamp: entry.created_at,
    });

    msg!(
        "Address {} whitelisted by {}",
        address,
        ctx.accounts.authority.key()
    );
    Ok(())
}

#[derive(Accounts)]
pub struct AddWhitelistEntry<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [WhitelistAuthority::SEED],
        bump,
        constraint = whitelist_authority.authority.is_some() @ TransferHookError::AuthorityAlreadyBurned
    )]
    pub whitelist_authority: Account<'info, WhitelistAuthority>,

    #[account(
        init,
        payer = authority,
        space = 8 + WhitelistEntry::INIT_SPACE,
        seeds = [WhitelistEntry::SEED_PREFIX, address_to_whitelist.key().as_ref()],
        bump
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,

    /// CHECK: Address being whitelisted. Can be any account (pool vault, etc).
    /// Validated in handler to reject system program and null pubkey.
    pub address_to_whitelist: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}
