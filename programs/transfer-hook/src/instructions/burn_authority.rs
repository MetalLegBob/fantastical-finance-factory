use crate::errors::TransferHookError;
use crate::events::AuthorityBurned;
use crate::state::WhitelistAuthority;
use anchor_lang::prelude::*;

/// Handler for burn_authority instruction.
///
/// Permanently disables the whitelist authority by setting authority to None.
/// This makes the whitelist immutable - no more entries can be added.
///
/// IDEMPOTENT: If authority is already burned, succeeds silently (returns Ok).
/// This allows safe retry logic and scripts to be re-run without error.
///
/// Order of checks is critical:
/// 1. Check if already burned → return Ok (idempotent behavior)
/// 2. Verify signer is current authority → Unauthorized if not
/// 3. Burn the authority
/// 4. Emit event
///
/// Spec reference: Transfer_Hook_Spec.md Section 6.3, 7.3
pub fn handler(ctx: Context<BurnAuthority>) -> Result<()> {
    let auth = &mut ctx.accounts.whitelist_authority;

    // IDEMPOTENT: If already burned, succeed silently (15-CONTEXT.md decision)
    // This check MUST come before authority validation
    if auth.authority.is_none() {
        msg!("Authority already burned, idempotent success");
        return Ok(());
    }

    // Now verify signer is the current authority
    require!(
        auth.authority == Some(ctx.accounts.authority.key()),
        TransferHookError::Unauthorized
    );

    // Burn the authority
    auth.authority = None;

    // Emit event (15-CONTEXT.md: "important milestone worth tracking")
    emit!(AuthorityBurned {
        burned_by: ctx.accounts.authority.key(),
        timestamp: Clock::get()?.unix_timestamp,
    });

    msg!(
        "Authority burned permanently by {}",
        ctx.accounts.authority.key()
    );
    Ok(())
}

/// Accounts for burn_authority instruction.
///
/// No system_program needed - we're modifying existing account, not creating.
#[derive(Accounts)]
pub struct BurnAuthority<'info> {
    /// Authority that will burn itself (must be current authority).
    pub authority: Signer<'info>,

    /// Whitelist authority PDA being burned.
    #[account(
        mut,
        seeds = [WhitelistAuthority::SEED],
        bump
    )]
    pub whitelist_authority: Account<'info, WhitelistAuthority>,
}
