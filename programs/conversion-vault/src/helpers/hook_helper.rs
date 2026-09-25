use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022;

use crate::error::VaultError;

/// Transfer Token-2022 tokens via manual `invoke_signed` CPI, forwarding hook accounts
/// from `remaining_accounts`.
///
/// # Why manual invoke_signed instead of Anchor's CPI helper?
///
/// Anchor's `token_interface::transfer_checked` with `with_remaining_accounts` does NOT
/// properly forward remaining_accounts through the nested CPI chain:
///   Vault -> Token-2022 -> Transfer Hook program
///
/// Token-2022 needs the hook accounts (ExtraAccountMetaList, whitelist PDAs, hook program)
/// to be present in both the instruction's account keys AND the account_infos passed to
/// `invoke_signed`. Anchor's CPI framework only adds them to the CpiContext but doesn't
/// ensure they appear in the raw instruction keys that Token-2022 reads.
///
/// This manual approach (identical to AMM's `transfer_t22_checked`) builds the raw
/// `spl_token_2022::instruction::transfer_checked` instruction, appends hook accounts
/// to both `ix.accounts` AND `account_infos`, then calls `invoke_signed`.
pub fn transfer_t22_checked<'info>(
    token_program: &AccountInfo<'info>,
    from: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    decimals: u8,
    signer_seeds: &[&[&[u8]]],
    hook_accounts: &[AccountInfo<'info>],
) -> Result<()> {
    // Defense-in-depth: verify token program is Token-2022.
    require!(
        *token_program.key == anchor_spl::token_2022::ID,
        VaultError::InvalidTokenProgram
    );

    // Defense-in-depth: reject zero-amount transfers early.
    require!(amount > 0, VaultError::ZeroAmount);

    // Build the base transfer_checked instruction with 4 standard accounts:
    // [from, mint, to, authority]
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

    // Append hook accounts to the instruction's account keys.
    // These are the Transfer Hook accounts resolved by the client:
    // - ExtraAccountMetaList PDA
    // - Whitelist PDA for source
    // - Whitelist PDA for destination
    // - Transfer Hook program
    for account_info in hook_accounts {
        ix.accounts.push(AccountMeta {
            pubkey: *account_info.key,
            is_signer: account_info.is_signer,
            is_writable: account_info.is_writable,
        });
    }

    // Build complete account_infos list: standard 4 + hook accounts.
    let mut account_infos = vec![from.clone(), mint.clone(), to.clone(), authority.clone()];
    for account_info in hook_accounts {
        account_infos.push(account_info.clone());
    }

    // Invoke with all accounts so Token-2022 can find hook accounts
    // during its nested CPI to the Transfer Hook program.
    anchor_lang::solana_program::program::invoke_signed(&ix, &account_infos, signer_seeds)?;

    Ok(())
}
