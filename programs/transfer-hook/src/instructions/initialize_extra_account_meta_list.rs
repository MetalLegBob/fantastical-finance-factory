use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;
use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use crate::errors::TransferHookError;
use crate::events::ExtraAccountMetaListInitialized;
use crate::state::WhitelistAuthority;

/// Initialize ExtraAccountMetaList for a mint.
///
/// Creates the PDA that Token-2022 uses to resolve whitelist accounts
/// at transfer time. Must be called once per mint before transfers.
///
/// Spec reference: Transfer_Hook_Spec.md Section 8
pub fn handler(ctx: Context<InitializeExtraAccountMetaList>) -> Result<()> {
    // Validate mint has correct transfer hook extension
    validate_mint_hook(&ctx.accounts.mint.to_account_info(), &crate::ID)?;

    // Validate authority (constraint ensures authority.is_some())
    require!(
        ctx.accounts.whitelist_authority.authority == Some(ctx.accounts.authority.key()),
        TransferHookError::Unauthorized
    );

    // Define extra accounts for transfer hook resolution:
    // Account indices per SPL Transfer Hook spec:
    // 0 = source_token_account, 1 = mint, 2 = destination_token_account, 3 = owner
    let extra_metas = vec![
        // Source whitelist PDA: ["whitelist", source_token_account]
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: b"whitelist".to_vec(),
                },
                Seed::AccountKey { index: 0 }, // source_token_account
            ],
            false, // is_signer
            false, // is_writable
        )?,
        // Destination whitelist PDA: ["whitelist", destination_token_account]
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: b"whitelist".to_vec(),
                },
                Seed::AccountKey { index: 2 }, // destination_token_account
            ],
            false, // is_signer
            false, // is_writable
        )?,
    ];

    // Calculate required account size and rent
    let account_size = ExtraAccountMetaList::size_of(extra_metas.len())?;
    let lamports = Rent::get()?.minimum_balance(account_size);

    // Create the ExtraAccountMetaList account via CPI
    let mint_key = ctx.accounts.mint.key();
    let signer_seeds: &[&[u8]] = &[
        b"extra-account-metas",
        mint_key.as_ref(),
        &[ctx.bumps.extra_account_meta_list],
    ];

    anchor_lang::system_program::create_account(
        CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::CreateAccount {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.extra_account_meta_list.to_account_info(),
            },
            &[signer_seeds],
        ),
        lamports,
        account_size as u64,
        &crate::ID,
    )?;

    // Initialize with TLV data
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?,
        &extra_metas,
    )?;

    emit!(ExtraAccountMetaListInitialized {
        mint: ctx.accounts.mint.key(),
    });

    msg!(
        "ExtraAccountMetaList initialized for mint {}",
        ctx.accounts.mint.key()
    );
    Ok(())
}

/// Validate mint is Token-2022 and has our program as transfer hook.
fn validate_mint_hook(mint_info: &AccountInfo, expected_hook_program: &Pubkey) -> Result<()> {
    use anchor_spl::token_2022;
    use spl_token_2022::extension::{transfer_hook, StateWithExtensions};
    use spl_token_2022::state::Mint as T22Mint;

    // Check 1: Mint must be owned by Token-2022 program
    require!(
        *mint_info.owner == token_2022::ID,
        TransferHookError::NotToken2022Mint
    );

    // Check 2: Mint must have TransferHook extension pointing to our program
    let mint_data = mint_info.try_borrow_data()?;
    let mint_state = StateWithExtensions::<T22Mint>::unpack(&mint_data)?;
    let hook_program_id = transfer_hook::get_program_id(&mint_state);

    require!(
        hook_program_id == Some(*expected_hook_program),
        TransferHookError::InvalidTransferHook
    );

    Ok(())
}

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        seeds = [WhitelistAuthority::SEED],
        bump,
        constraint = whitelist_authority.authority.is_some() @ TransferHookError::AuthorityAlreadyBurned
    )]
    pub whitelist_authority: Account<'info, WhitelistAuthority>,

    /// Authority that controls whitelist operations. Must match WhitelistAuthority.authority.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: Validated via seeds constraint. Will be initialized by create_account CPI.
    /// Seeds: ["extra-account-metas", mint.key()]
    #[account(
        mut,
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump,
    )]
    pub extra_account_meta_list: UncheckedAccount<'info>,

    /// The mint for which to initialize ExtraAccountMetaList.
    /// Must be Token-2022 with TransferHook extension pointing to this program.
    pub mint: InterfaceAccount<'info, Mint>,

    pub system_program: Program<'info, System>,
}
