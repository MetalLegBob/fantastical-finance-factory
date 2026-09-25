use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use spl_token_2022::extension::{
    transfer_hook::TransferHookAccount, BaseStateWithExtensions, PodStateWithExtensions,
};
use spl_token_2022::pod::PodAccount;

use crate::errors::TransferHookError;
use crate::state::WhitelistEntry;

/// Accounts for the transfer_hook instruction.
///
/// Token-2022 invokes this during transfer_checked for mints with transfer hook extension.
/// Account indices follow SPL Transfer Hook specification:
/// - 0: source_token_account
/// - 1: mint
/// - 2: destination_token_account
/// - 3: owner/authority
/// - 4: extra_account_meta_list
/// - 5+: resolved extra accounts (whitelist PDAs from ExtraAccountMetaList)
///
/// Spec reference: Transfer_Hook_Spec.md Section 7.4
#[derive(Accounts)]
pub struct TransferHook<'info> {
    /// Source token account (SPL account index 0)
    #[account(token::mint = mint)]
    pub source_token: InterfaceAccount<'info, TokenAccount>,

    /// Token mint (SPL account index 1)
    pub mint: InterfaceAccount<'info, Mint>,

    /// Destination token account (SPL account index 2)
    #[account(token::mint = mint)]
    pub destination_token: InterfaceAccount<'info, TokenAccount>,

    /// Source token owner/authority (SPL account index 3)
    /// CHECK: Validated by Token-2022 before calling hook
    pub owner: UncheckedAccount<'info>,

    /// ExtraAccountMetaList PDA (SPL account index 4)
    /// CHECK: Validated via seeds constraint
    #[account(
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list: UncheckedAccount<'info>,

    /// Whitelist PDA for source token account (extra account index 5)
    /// Resolved from ExtraAccountMetaList: ["whitelist", source_token.key()]
    /// CHECK: Existence and derivation checked in handler
    pub whitelist_source: UncheckedAccount<'info>,

    /// Whitelist PDA for destination token account (extra account index 6)
    /// Resolved from ExtraAccountMetaList: ["whitelist", destination_token.key()]
    /// CHECK: Existence and derivation checked in handler
    pub whitelist_destination: UncheckedAccount<'info>,
}

/// Handler for transfer_hook instruction.
///
/// Validates that at least one party (source or destination) is whitelisted.
/// Called by Token-2022 during transfer_checked.
///
/// # Validation Order (per CONTEXT.md)
/// 1. Zero amount check (cheapest, fail fast)
/// 2. Mint owner check (defense-in-depth, per CONTEXT.md SECU-02)
/// 3. Transferring flag check (security - verify legitimate T22 context)
/// 4. Whitelist check with short-circuit (business rule)
///
/// # Arguments
/// * `ctx` - The accounts context
/// * `amount` - Transfer amount (passed by Token-2022)
///
/// # Errors
/// - ZeroAmountTransfer: Amount is zero
/// - InvalidMint: Mint not owned by Token-2022 (defense-in-depth)
/// - DirectInvocationNotAllowed: Not called from Token-2022 transfer context
/// - NoWhitelistedParty: Neither source nor destination is whitelisted
pub fn handler(ctx: Context<TransferHook>, amount: u64) -> Result<()> {
    // Validation 1: Zero amount check (cheapest, fail fast)
    // SECU-03: Zero amount transfers rejected
    require!(amount > 0, TransferHookError::ZeroAmountTransfer);

    // Validation 2: Mint owner check (defense-in-depth)
    // SECU-02: Mint must be owned by Token-2022 program
    // Note: ExtraAccountMetaList provides implicit validation, but we add this
    // explicit check as defense-in-depth per CONTEXT.md decision
    check_mint_owner(&ctx.accounts.mint.to_account_info())?;

    // Validation 3: Transferring flag check (security)
    // SECU-01: Prevents direct hook invocation attack
    check_is_transferring(&ctx.accounts.source_token.to_account_info())?;

    // Validation 4: Whitelist check with short-circuit
    // WHTE-06: Transfer allowed if source OR destination is whitelisted
    // WHTE-07: Transfer blocked if neither is whitelisted
    // SECU-04: PDA derivation verified (prevents spoofed accounts)
    let source_whitelisted = is_whitelisted(
        &ctx.accounts.whitelist_source.to_account_info(),
        &ctx.accounts.source_token.key(),
    );

    if !source_whitelisted {
        // Only check destination if source is not whitelisted (short-circuit optimization)
        let dest_whitelisted = is_whitelisted(
            &ctx.accounts.whitelist_destination.to_account_info(),
            &ctx.accounts.destination_token.key(),
        );

        require!(dest_whitelisted, TransferHookError::NoWhitelistedParty);
    }

    // Transfer allowed - Token-2022 will complete the transfer
    Ok(())
}

/// Check that the mint is owned by the Token-2022 program.
///
/// Defense-in-depth: Even though ExtraAccountMetaList provides implicit mint
/// validation (only mints with initialized ExtraAccountMetaList can invoke hook),
/// we add this explicit check per CONTEXT.md SECU-02 requirements.
///
/// # Errors
/// - InvalidMint: mint.owner != spl_token_2022::id()
fn check_mint_owner(mint: &AccountInfo) -> Result<()> {
    // Verify mint is owned by Token-2022 program
    require!(
        mint.owner == &spl_token_2022::id(),
        TransferHookError::InvalidMint
    );
    Ok(())
}

/// Check that we're being called from a legitimate Token-2022 transfer.
///
/// The transferring flag is set by Token-2022 before calling the hook and
/// unset after the hook returns. If this flag is not set, someone is trying
/// to invoke the hook directly (attack vector).
///
/// # Errors
/// - DirectInvocationNotAllowed: transferring flag is false
fn check_is_transferring(source_token: &AccountInfo) -> Result<()> {
    let account_data = source_token.try_borrow_data()?;
    let account = PodStateWithExtensions::<PodAccount>::unpack(&account_data)
        .map_err(|_| TransferHookError::DirectInvocationNotAllowed)?;

    let extension = account
        .get_extension::<TransferHookAccount>()
        .map_err(|_| TransferHookError::DirectInvocationNotAllowed)?;

    if !bool::from(extension.transferring) {
        return err!(TransferHookError::DirectInvocationNotAllowed);
    }

    Ok(())
}

/// Check if a token account is whitelisted.
///
/// Uses existence-based PDA pattern: PDA exists = whitelisted.
/// Also verifies PDA derivation to prevent spoofed accounts.
///
/// # Arguments
/// * `whitelist_pda` - The whitelist PDA account passed to the instruction
/// * `token_account` - The token account pubkey to check
///
/// # Returns
/// true if the whitelist PDA exists and has correct derivation, false otherwise
fn is_whitelisted(whitelist_pda: &AccountInfo, token_account: &Pubkey) -> bool {
    // Account must have data (exists)
    if whitelist_pda.data_is_empty() {
        return false;
    }

    // Verify PDA derivation is correct (SECU-04: prevents spoofed accounts)
    let (expected_pda, _bump) = Pubkey::find_program_address(
        &[WhitelistEntry::SEED_PREFIX, token_account.as_ref()],
        &crate::ID,
    );

    whitelist_pda.key() == expected_pda
}
