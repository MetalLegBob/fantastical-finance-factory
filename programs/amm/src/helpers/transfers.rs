use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022;
use anchor_spl::token_interface::{self, TransferChecked};

use crate::errors::AmmError;

/// Transfer Token-2022 tokens via manual `invoke_signed` CPI, forwarding hook accounts
/// from `remaining_accounts`.
///
/// # Why manual invoke_signed instead of Anchor's CPI helper?
///
/// Anchor's `token_interface::transfer_checked` with `with_remaining_accounts` does NOT
/// properly forward remaining_accounts through the nested CPI chain:
///   AMM -> Token-2022 -> Transfer Hook program
///
/// Token-2022 needs the hook accounts (ExtraAccountMetaList, whitelist PDAs, hook program)
/// to be present in both the instruction's account keys AND the account_infos passed to
/// `invoke_signed`. Anchor's CPI framework only adds them to the CpiContext but doesn't
/// ensure they appear in the raw instruction keys that Token-2022 reads.
///
/// This manual approach (identical to staking program's `transfer_checked_with_hook`)
/// builds the raw `spl_token_2022::instruction::transfer_checked` instruction, appends
/// hook accounts to both `ix.accounts` AND `account_infos`, then calls `invoke_signed`.
///
/// # When to use
/// Any transfer of Token-2022 tokens (CRIME, FRAUD, PROFIT). These tokens have the
/// Transfer Hook extension, which requires hook accounts to be appended to the CPI
/// instruction's account list.
///
/// # Validation (defense-in-depth)
/// - **Token program:** Requires the token program to be Token-2022 (`spl_token_2022::ID`).
///   This prevents accidentally calling an arbitrary program via CPI.
/// - **Amount:** Requires amount > 0. Zero-amount transfers waste compute and indicate
///   a logic bug in the calling instruction handler.
///
/// # Signer seeds
/// - **Empty (`&[]`):** User-signed transfer (user-to-vault). The user's wallet is the
///   authority and signs the transaction directly.
/// - **Populated:** PDA-signed transfer (vault-to-user). The pool PDA is the authority
///   and signs via `invoke_signed`.
///
/// # Hook accounts
/// The caller (instruction handler) pre-resolves ExtraAccountMetas off-chain and passes
/// them as `remaining_accounts`. This function appends them to both the instruction keys
/// and account_infos before calling `invoke_signed`. Token-2022 internally CPIs to the
/// hook program with these accounts. The AMM never interprets hook logic -- it just
/// passes the right accounts.
///
/// # Security
/// - Always uses `transfer_checked` (never plain `transfer`). Plain `transfer` silently
///   bypasses T22 hooks, which would skip whitelist enforcement.
/// - Token program ID is validated before CPI to prevent calling arbitrary programs.
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
    // Anchor account constraints validate this at the instruction level,
    // but we double-check here to prevent misuse if helpers are called
    // from new instruction handlers that lack proper constraints.
    require!(
        *token_program.key == anchor_spl::token_2022::ID,
        AmmError::InvalidTokenProgram
    );

    // Defense-in-depth: reject zero-amount transfers early.
    // Zero amounts waste compute and indicate a bug in swap math.
    require!(amount > 0, AmmError::ZeroAmount);

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
    // Both ix.accounts and account_infos must contain the hook accounts
    // for Token-2022's nested CPI to the Transfer Hook program to succeed.
    let mut account_infos = vec![from.clone(), mint.clone(), to.clone(), authority.clone()];
    for account_info in hook_accounts {
        account_infos.push(account_info.clone());
    }

    // Invoke with all accounts so Token-2022 can find hook accounts
    // during its nested CPI to the Transfer Hook program.
    anchor_lang::solana_program::program::invoke_signed(&ix, &account_infos, signer_seeds)?;

    Ok(())
}

/// Transfer SPL Token tokens via `transfer_checked` CPI (no hook accounts).
///
/// # When to use
/// Any transfer of standard SPL Token tokens (WSOL). SPL Token does not support
/// transfer hooks, so no hook accounts are needed.
///
/// # Validation (defense-in-depth)
/// - **Token program:** Requires the token program to be SPL Token (`spl_token::ID`).
///   This prevents accidentally sending SPL tokens through Token-2022 (which would fail
///   with `IncorrectProgramId`) or calling an arbitrary program.
/// - **Amount:** Requires amount > 0. Zero-amount transfers waste compute and indicate
///   a logic bug in the calling instruction handler.
///
/// # Signer seeds
/// - **Empty (`&[]`):** User-signed transfer (user-to-vault). The user's wallet is the
///   authority and signs the transaction directly.
/// - **Populated:** PDA-signed transfer (vault-to-user). The pool PDA is the authority
///   and signs via `CpiContext::new_with_signer`.
///
/// # Security
/// - Uses `transfer_checked` for consistency with the T22 helper, even though SPL Token
///   does not have hooks. `transfer_checked` validates the mint and decimals, providing
///   an extra layer of safety.
/// - Token program ID is validated before CPI to prevent calling arbitrary programs.
pub fn transfer_spl<'info>(
    token_program: &AccountInfo<'info>,
    from: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    decimals: u8,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    // Defense-in-depth: verify token program is SPL Token.
    // Calling Token-2022 with SPL Token accounts would fail at the CPI level,
    // but catching it here gives a clearer error message.
    require!(
        *token_program.key == anchor_spl::token::ID,
        AmmError::InvalidTokenProgram
    );

    // Defense-in-depth: reject zero-amount transfers early.
    require!(amount > 0, AmmError::ZeroAmount);

    let cpi_accounts = TransferChecked {
        from: from.clone(),
        mint: mint.clone(),
        to: to.clone(),
        authority: authority.clone(),
    };

    // Build CPI context: use signer if PDA-signed (vault-to-user),
    // plain context if user-signed (user-to-vault).
    let cpi_ctx = if signer_seeds.is_empty() {
        CpiContext::new(token_program.clone(), cpi_accounts)
    } else {
        CpiContext::new_with_signer(token_program.clone(), cpi_accounts, signer_seeds)
    };

    // No remaining_accounts -- SPL Token has no hook support.
    token_interface::transfer_checked(cpi_ctx, amount, decimals)
}
