//! Generic SPL quote -> CRIME/FRAUD taxed swap lane.
//!
//! CEI order mirrors the deployed donor `swap_sol_buy.rs`: validate EpochState
//! and pause gates, derive faction identity/orientation from the pool, calculate
//! tax and the oriented post-tax floor, skim quote tax, invoke the AMM, reload
//! the faction output account, check realized output, then emit the accounting
//! event. This lane has no WSOL wrapping or inline split machinery.
//!
//! CPI depth: user transaction -> Tax -> AMM (1) -> Token-2022 (2) -> hook (3),
//! leaving one level of headroom. The lane-side quote skim is depth 1.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use anchor_lang::AccountDeserialize;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::constants::{
    amm_program_id, epoch_program_id, MINIMUM_OUTPUT_FLOOR_BPS, SWAP_AUTHORITY_SEED,
    SWEEP_AUTHORITY_SEED,
};
use crate::errors::TaxError;
use crate::events::SplSwapExecuted;
use crate::helpers::faction_pool_reader;
use crate::helpers::tax_math::{calculate_output_floor, calculate_tax};
use crate::state::EpochState;

/// Read the live base amount from either an SPL Token or Token-2022 account.
fn token_amount(account: &AccountInfo<'_>) -> Result<u64> {
    let data = account.try_borrow_data()?;
    let mut data_slice: &[u8] = &data;
    Ok(TokenAccount::try_deserialize(&mut data_slice)?.amount)
}

/// Execute a generic non-SOL quote -> faction swap with input-side tax.
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SwapSplBuy<'info>>,
    amount_in: u64,
    minimum_output: u64,
) -> Result<()> {
    require!(amount_in > 0, TaxError::InsufficientInput);

    // Tax is calculated from the nominal debit, so the quote mint must credit
    // both the sweep account and AMM vault in full. This repeats the AMM guard
    // before Tax performs its own transfer.
    amm::helpers::mint_policy::validate_amount_preserving_mint(
        &ctx.accounts.quote_mint.to_account_info(),
    )
    .map_err(|_| error!(TaxError::UnsupportedTransferFee))?;

    // Owner check prevents a caller-provided state account from selecting a
    // forged zero tax rate.
    let epoch_program = epoch_program_id();
    require!(
        ctx.accounts.epoch_state.owner == &epoch_program,
        TaxError::InvalidEpochState
    );

    let epoch_state = {
        let data = ctx.accounts.epoch_state.try_borrow_data()?;
        let mut data_slice: &[u8] = &data;
        EpochState::try_deserialize(&mut data_slice)
            .map_err(|_| error!(TaxError::InvalidEpochState))?
    };

    require!(epoch_state.initialized, TaxError::InvalidEpochState);

    // TAX-01 pause gates (Phase 167). Order ratified: manual pause FIRST (§14.1-7).
    require!(!epoch_state.trading_paused, TaxError::TradingPauseActive);
    // Inclusive per spec §4.1: slot >= pause_end_slot trades. Pre-upgrade bytes give
    // pause_end_slot == 0 -> always trades (no special case needed).
    require!(
        Clock::get()?.slot >= epoch_state.pause_end_slot,
        TaxError::EpochPauseActive
    );

    // Both faction identities are validated inside the reader. The resulting
    // orientation and is_crime value are never caller-controlled.
    let view = faction_pool_reader::read_faction_pool(&ctx.accounts.pool)?;
    require_keys_eq!(
        ctx.accounts.quote_mint.key(),
        view.quote_mint,
        TaxError::NotAFactionPool
    );

    let tax_bps = epoch_state.get_tax_bps(view.is_crime, true);
    let tax_amount = calculate_tax(amount_in, tax_bps).ok_or(error!(TaxError::TaxOverflow))?;
    let swap_input = amount_in
        .checked_sub(tax_amount)
        .ok_or(error!(TaxError::TaxOverflow))?;
    require!(swap_input > 0, TaxError::InsufficientInput);

    // Buy floor is oriented quote -> faction and uses the post-tax AMM input.
    let output_floor = calculate_output_floor(
        view.quote_reserve,
        view.faction_reserve,
        swap_input,
        MINIMUM_OUTPUT_FLOOR_BPS,
    )
    .ok_or(error!(TaxError::TaxOverflow))?;
    require!(
        minimum_output >= output_floor,
        TaxError::MinimumOutputFloorViolation
    );

    // The quote side is the non-faction side. transfer_checked binds its mint
    // and decimals to the pool-derived quote mint and constrained sweep ATA.
    let user_quote = if view.faction_is_a {
        &ctx.accounts.user_token_b
    } else {
        &ctx.accounts.user_token_a
    };
    token_interface::transfer_checked(
        CpiContext::new(
            ctx.accounts.quote_token_program.to_account_info(),
            TransferChecked {
                from: user_quote.clone(),
                mint: ctx.accounts.quote_mint.to_account_info(),
                to: ctx.accounts.sweep_ata.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        tax_amount,
        ctx.accounts.quote_mint.decimals,
    )?;

    let user_faction = if view.faction_is_a {
        &ctx.accounts.user_token_a
    } else {
        &ctx.accounts.user_token_b
    };
    let faction_before = token_amount(user_faction)?;

    let direction: u8 = if view.faction_is_a {
        1 // BtoA: quote B in, faction A out
    } else {
        0 // AtoB: quote A in, faction B out
    };
    let swap_authority_seeds: &[&[u8]] = &[SWAP_AUTHORITY_SEED, &[ctx.bumps.swap_authority]];

    // Account order is the AMM SwapSolPool positional contract.
    let mut account_metas = vec![
        AccountMeta::new_readonly(ctx.accounts.swap_authority.key(), true),
        AccountMeta::new(ctx.accounts.pool.key(), false),
        AccountMeta::new(ctx.accounts.vault_a.key(), false),
        AccountMeta::new(ctx.accounts.vault_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_b.key(), false),
        AccountMeta::new(ctx.accounts.user_token_a.key(), false),
        AccountMeta::new(ctx.accounts.user_token_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.user.key(), true),
        AccountMeta::new_readonly(ctx.accounts.token_program_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.token_program_b.key(), false),
    ];

    // Transfer-hook accounts must appear in both the instruction metas and the
    // invoke account infos, preserving their signer and writable privileges.
    for account in ctx.remaining_accounts.iter() {
        if account.is_writable {
            account_metas.push(AccountMeta::new(account.key(), account.is_signer));
        } else {
            account_metas.push(AccountMeta::new_readonly(account.key(), account.is_signer));
        }
    }

    const AMM_SWAP_SOL_POOL_DISCRIMINATOR: [u8; 8] =
        [0xde, 0x80, 0x1e, 0x7b, 0x55, 0x27, 0x91, 0x8a];
    let mut ix_data = Vec::with_capacity(25);
    ix_data.extend_from_slice(&AMM_SWAP_SOL_POOL_DISCRIMINATOR);
    ix_data.extend_from_slice(&swap_input.to_le_bytes());
    ix_data.push(direction);
    ix_data.extend_from_slice(&minimum_output.to_le_bytes());

    let ix = Instruction {
        program_id: ctx.accounts.amm_program.key(),
        accounts: account_metas,
        data: ix_data,
    };

    let mut account_infos = vec![
        ctx.accounts.swap_authority.to_account_info(),
        ctx.accounts.pool.to_account_info(),
        ctx.accounts.vault_a.to_account_info(),
        ctx.accounts.vault_b.to_account_info(),
        ctx.accounts.mint_a.to_account_info(),
        ctx.accounts.mint_b.to_account_info(),
        ctx.accounts.user_token_a.to_account_info(),
        ctx.accounts.user_token_b.to_account_info(),
        ctx.accounts.user.to_account_info(),
        ctx.accounts.token_program_a.to_account_info(),
        ctx.accounts.token_program_b.to_account_info(),
    ];
    for account in ctx.remaining_accounts.iter() {
        account_infos.push(account.clone());
    }
    account_infos.push(ctx.accounts.amm_program.to_account_info());

    invoke_signed(&ix, &account_infos, &[swap_authority_seeds])?;

    // Re-deserialize after CPI; no pre-CPI cached balance is reused (FIX-06).
    let faction_after = token_amount(user_faction)?;
    let amount_out = faction_after
        .checked_sub(faction_before)
        .ok_or(error!(TaxError::TaxOverflow))?;
    require!(amount_out >= minimum_output, TaxError::SlippageExceeded);

    emit!(SplSwapExecuted {
        pool: ctx.accounts.pool.key(),
        quote_mint: view.quote_mint,
        is_crime: view.is_crime,
        direction,
        amount_in,
        tax_amount,
        amount_out,
        tax_rate_bps: tax_bps,
        epoch: epoch_state.current_epoch,
    });

    Ok(())
}

/// Positional accounts for the generic quote -> faction taxed lane.
#[derive(Accounts)]
pub struct SwapSplBuy<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    /// CHECK: Owner, discriminator, and initialized state are checked in the handler.
    pub epoch_state: AccountInfo<'info>,

    /// CHECK: Owner and faction identity are validated by read_faction_pool.
    #[account(mut)]
    pub pool: AccountInfo<'info>,

    /// CHECK: The AMM validates the canonical vault keys from pool state.
    #[account(mut)]
    pub vault_a: AccountInfo<'info>,

    /// CHECK: The AMM validates the canonical vault keys from pool state.
    #[account(mut)]
    pub vault_b: AccountInfo<'info>,

    /// CHECK: The AMM validates the canonical mint keys from pool state.
    pub mint_a: AccountInfo<'info>,

    /// CHECK: The AMM validates the canonical mint keys from pool state.
    pub mint_b: AccountInfo<'info>,

    /// CHECK: The AMM token CPI validates mint and user authority.
    #[account(mut)]
    pub user_token_a: AccountInfo<'info>,

    /// CHECK: The AMM token CPI validates mint and user authority.
    #[account(mut)]
    pub user_token_b: AccountInfo<'info>,

    /// CHECK: Seed-constrained Tax PDA; signs the AMM CPI.
    #[account(seeds = [SWAP_AUTHORITY_SEED], bump)]
    pub swap_authority: AccountInfo<'info>,

    /// CHECK: Seed-constrained Tax PDA; owns every per-quote sweep ATA.
    #[account(seeds = [SWEEP_AUTHORITY_SEED], bump)]
    pub sweep_authority: AccountInfo<'info>,

    pub quote_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = quote_mint,
        associated_token::authority = sweep_authority,
        associated_token::token_program = quote_token_program,
    )]
    pub sweep_ata: InterfaceAccount<'info, TokenAccount>,

    pub quote_token_program: Interface<'info, TokenInterface>,

    /// CHECK: Forwarded positionally; the AMM validates it against mint A.
    pub token_program_a: AccountInfo<'info>,

    /// CHECK: Forwarded positionally; the AMM validates it against mint B.
    pub token_program_b: AccountInfo<'info>,

    /// CHECK: Canonical AMM address is enforced here.
    #[account(address = amm_program_id() @ TaxError::InvalidAmmProgram)]
    pub amm_program: AccountInfo<'info>,
}
