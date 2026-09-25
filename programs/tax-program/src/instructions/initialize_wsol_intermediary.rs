//! One-time initialization of the WSOL intermediary PDA account.
//!
//! Called during protocol deployment (admin-only).
//! Creates the WSOL intermediary at a PDA address owned by swap_authority,
//! so that swap_sol_sell can use it for the transfer-close-distribute-reinit
//! tax flow.
//!
//! This must be called BEFORE the first sell swap. The intermediary is a
//! WSOL (Native Mint) token account at a PDA derived from the Tax Program.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
    system_instruction,
};

use crate::constants::{SWAP_AUTHORITY_SEED, WSOL_INTERMEDIARY_SEED};

/// Initialize the WSOL intermediary PDA as a WSOL token account.
///
/// Steps:
/// 1. create_account at PDA address (admin pays rent, intermediary PDA signs)
/// 2. InitializeAccount3 to set up as WSOL token account owned by swap_authority
///
/// # Notes
/// - Admin is the payer (funds the rent-exempt minimum)
/// - The intermediary PDA must sign create_account via invoke_signed
/// - swap_authority is set as the token account owner (for close_account in sell flow)
/// - Uses InitializeAccount3 (discriminator 18) which takes owner as data, no rent sysvar
pub fn handler(ctx: Context<InitializeWsolIntermediary>) -> Result<()> {
    let rent = Rent::get()?;
    let space = 165u64; // spl_token::state::Account::LEN
    let rent_lamports = rent.minimum_balance(space as usize);

    // Step 1: Create account at PDA address
    let intermediary_seeds: &[&[u8]] = &[WSOL_INTERMEDIARY_SEED, &[ctx.bumps.wsol_intermediary]];

    let create_ix = system_instruction::create_account(
        ctx.accounts.admin.key,
        ctx.accounts.wsol_intermediary.key,
        rent_lamports,
        space,
        &ctx.accounts.token_program.key(),
    );

    invoke_signed(
        &create_ix,
        &[
            ctx.accounts.admin.to_account_info(),
            ctx.accounts.wsol_intermediary.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
        &[intermediary_seeds],
    )?;

    // Step 2: Initialize as WSOL token account using InitializeAccount3
    // InitializeAccount3 (discriminator 18) does NOT require rent sysvar.
    // Owner is passed as instruction data (32 bytes after discriminator).
    let init_ix = Instruction {
        program_id: ctx.accounts.token_program.key(),
        accounts: vec![
            AccountMeta::new(ctx.accounts.wsol_intermediary.key(), false),
            AccountMeta::new_readonly(ctx.accounts.mint.key(), false),
        ],
        data: {
            let mut d = vec![18u8]; // InitializeAccount3 discriminator
            d.extend_from_slice(&ctx.accounts.swap_authority.key().to_bytes());
            d
        },
    };

    invoke(
        &init_ix,
        &[
            ctx.accounts.wsol_intermediary.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    msg!(
        "WSOL intermediary initialized at {}",
        ctx.accounts.wsol_intermediary.key()
    );
    Ok(())
}

/// Accounts for the one-time WSOL intermediary initialization.
///
/// Called by admin during protocol deployment, before the first sell swap.
/// Creates a WSOL token account at the intermediary PDA, owned by swap_authority.
#[derive(Accounts)]
pub struct InitializeWsolIntermediary<'info> {
    /// Admin (payer). Only needs to be called once during protocol setup.
    #[account(mut)]
    pub admin: Signer<'info>,

    /// WSOL intermediary PDA -- must not exist yet.
    /// CHECK: PDA derived from seeds. Will be created in handler.
    #[account(
        mut,
        seeds = [WSOL_INTERMEDIARY_SEED],
        bump,
    )]
    pub wsol_intermediary: AccountInfo<'info>,

    /// swap_authority PDA -- will be set as the owner of the WSOL token account.
    /// CHECK: PDA derived from seeds.
    #[account(
        seeds = [SWAP_AUTHORITY_SEED],
        bump,
    )]
    pub swap_authority: AccountInfo<'info>,

    /// WSOL mint (NATIVE_MINT).
    /// CHECK: Must be native mint. Validated by SPL Token InitializeAccount.
    pub mint: AccountInfo<'info>,

    /// SPL Token program.
    /// CHECK: Well-known program.
    pub token_program: AccountInfo<'info>,

    /// The Tax Program — used to look up its ProgramData address.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    pub program: Program<'info, crate::program::TaxProgram>,

    /// ProgramData account — upgrade_authority must match admin.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(admin.key())
    )]
    pub program_data: Account<'info, ProgramData>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}
