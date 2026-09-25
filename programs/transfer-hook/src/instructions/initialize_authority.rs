use anchor_lang::prelude::*;

use crate::state::WhitelistAuthority;

/// Initializes the WhitelistAuthority PDA.
///
/// Creates the global whitelist authority account with the transaction signer
/// as the authority. This instruction can only be called once because `init`
/// will fail if the WhitelistAuthority PDA already exists.
///
/// The authority can later add whitelist entries or burn their authority
/// to make the whitelist immutable.
///
/// Spec reference: Transfer_Hook_Spec.md Section 7.1
pub fn handler(ctx: Context<InitializeAuthority>) -> Result<()> {
    let auth = &mut ctx.accounts.whitelist_authority;
    auth.authority = Some(ctx.accounts.signer.key());
    auth.initialized = true;

    msg!(
        "WhitelistAuthority initialized. Authority: {}",
        ctx.accounts.signer.key()
    );
    Ok(())
}

#[derive(Accounts)]
pub struct InitializeAuthority<'info> {
    /// The transaction signer who will become the whitelist authority.
    /// Must be mutable to pay for account creation.
    #[account(mut)]
    pub signer: Signer<'info>,

    /// The WhitelistAuthority PDA. Initialized once; stores the authority pubkey.
    /// Seeds: [b"authority"]
    #[account(
        init,
        payer = signer,
        space = 8 + WhitelistAuthority::INIT_SPACE,
        seeds = [WhitelistAuthority::SEED],
        bump
    )]
    pub whitelist_authority: Account<'info, WhitelistAuthority>,

    /// The Transfer Hook program — used to look up its ProgramData address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::TransferHook>,

    /// ProgramData account — upgrade_authority must match signer.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(signer.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    pub system_program: Program<'info, System>,
}
