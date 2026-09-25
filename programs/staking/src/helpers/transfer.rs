//! Transfer helpers for Token-2022 with Transfer Hook support.
//!
//! Anchor's `transfer_checked` CPI helper does not forward remaining_accounts
//! to Token-2022's `invoke_signed`. This means Transfer Hook extra accounts
//! (ExtraAccountMetaList, whitelist PDAs, hook program) are not passed through.
//!
//! This module provides `transfer_checked_with_hook` which manually builds the
//! CPI instruction and includes all remaining_accounts in both the instruction
//! keys and the account_infos passed to `invoke_signed`.
//!
//! Reference: https://github.com/coral-xyz/anchor/issues/anchor-spl-token-hook
//! Source: Phase 28 token-flow integration discovery

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022;

/// Perform a transfer_checked CPI to Token-2022 that includes remaining_accounts
/// for Transfer Hook support.
///
/// This is necessary because Anchor's built-in `transfer_checked` helper does not
/// forward remaining_accounts to `invoke_signed`, which means Token-2022 can't
/// find the hook accounts during transfer.
///
/// # Arguments
/// * `token_program` - Token-2022 program AccountInfo
/// * `from` - Source token account
/// * `mint` - Token mint
/// * `to` - Destination token account
/// * `authority` - Authority/owner of source account
/// * `remaining_accounts` - Hook accounts (ExtraAccountMetas, whitelist PDAs, hook program)
/// * `amount` - Amount to transfer
/// * `decimals` - Token decimals
/// * `signer_seeds` - Seeds for PDA signing (empty slice for non-PDA authority)
pub fn transfer_checked_with_hook<'info>(
    token_program: &AccountInfo<'info>,
    from: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    remaining_accounts: &[AccountInfo<'info>],
    amount: u64,
    decimals: u8,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    // Build the base transfer_checked instruction with 4 standard accounts
    let mut ix = spl_token_2022::instruction::transfer_checked(
        token_program.key,
        from.key,
        mint.key,
        to.key,
        authority.key,
        &[], // no multisig signers
        amount,
        decimals,
    )?;

    // Add remaining_accounts to the instruction keys
    // These are the Transfer Hook accounts resolved by the client:
    // - Whitelist PDA for source
    // - Whitelist PDA for destination
    // - Transfer Hook program
    // - ExtraAccountMetaList PDA
    for account_info in remaining_accounts {
        ix.accounts.push(AccountMeta {
            pubkey: *account_info.key,
            is_signer: account_info.is_signer,
            is_writable: account_info.is_writable,
        });
    }

    // Build complete account_infos list: standard 4 + remaining
    let mut account_infos = vec![from.clone(), mint.clone(), to.clone(), authority.clone()];
    for account_info in remaining_accounts {
        account_infos.push(account_info.clone());
    }

    // Invoke with all accounts so Token-2022 can find hook accounts
    anchor_lang::solana_program::program::invoke_signed(&ix, &account_infos, signer_seeds)?;

    Ok(())
}
