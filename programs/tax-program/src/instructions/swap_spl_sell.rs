//! Generic CRIME/FRAUD -> SPL quote taxed swap lane.
//!
//! The AMM sends gross quote output directly to the canonical sweep ATA. The
//! handler reloads that ATA, computes gross output from its balance delta (so
//! pre-existing swept tax is never counted), leaves tax in place, and forwards
//! only the net amount to the dedicated user quote destination in one
//! sweep-authority-signed transfer.
//!
//! CPI depth: user transaction -> Tax -> AMM (1) -> Token-2022 (2) -> hook (3),
//! leaving one level of headroom. The lane-side net forward is depth 1.

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

/// Execute a generic faction -> non-SOL quote swap with output-side tax.
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SwapSplSell<'info>>,
    amount_in: u64,
    minimum_output: u64,
) -> Result<()> {
    require!(amount_in > 0, TaxError::InsufficientInput);

    // The gross-output delta, output tax, forwarded net amount, and user
    // minimum all rely on one nominal quote unit equalling one credited unit.
    // Reject both current and future nonzero fee schedules before the CPI.
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

    // The AMM quote-output slot is deliberately the sweep ATA. The faction
    // slot remains the user's signed input account.
    let (amm_quote, user_faction) = if view.faction_is_a {
        (&ctx.accounts.user_token_b, &ctx.accounts.user_token_a)
    } else {
        (&ctx.accounts.user_token_a, &ctx.accounts.user_token_b)
    };
    require_keys_eq!(
        amm_quote.key(),
        ctx.accounts.sweep_ata.key(),
        TaxError::NotAFactionPool
    );
    require!(
        user_faction.key() != ctx.accounts.sweep_ata.key(),
        TaxError::NotAFactionPool
    );

    let tax_bps = epoch_state.get_tax_bps(view.is_crime, false);

    // Sell floor is oriented faction -> quote. Match the deployed SOL-sell
    // comparison: the user's post-tax minimum must meet the protocol floor.
    let output_floor = calculate_output_floor(
        view.faction_reserve,
        view.quote_reserve,
        amount_in,
        MINIMUM_OUTPUT_FLOOR_BPS,
    )
    .ok_or(error!(TaxError::TaxOverflow))?;
    require!(
        minimum_output >= output_floor,
        TaxError::MinimumOutputFloorViolation
    );

    // Compute the gross AMM floor required to leave minimum_output after tax:
    // ceil(minimum_output * 10_000 / (10_000 - tax_bps)). This is forwarded
    // into the AMM so insufficient output reverts before quote tokens move.
    let bps_denom: u64 = 10_000;
    let gross_floor = if minimum_output > 0 && (tax_bps as u64) < bps_denom {
        let numerator = (minimum_output as u128)
            .checked_mul(bps_denom as u128)
            .ok_or(error!(TaxError::TaxOverflow))?;
        let denominator = (bps_denom as u128)
            .checked_sub(tax_bps as u128)
            .ok_or(error!(TaxError::TaxOverflow))?;
        let result = numerator
            .checked_add(denominator - 1)
            .ok_or(error!(TaxError::TaxOverflow))?
            / denominator;
        u64::try_from(result).map_err(|_| error!(TaxError::TaxOverflow))?
    } else {
        0
    };

    // The sweep ATA may already contain tax from earlier swaps. Only its CPI
    // balance delta is this swap's gross quote output.
    let sweep_before = ctx.accounts.sweep_ata.amount;

    let direction: u8 = if view.faction_is_a {
        0 // AtoB: faction A in, quote B out
    } else {
        1 // BtoA: faction B in, quote A out
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
    ix_data.extend_from_slice(&amount_in.to_le_bytes());
    ix_data.push(direction);
    ix_data.extend_from_slice(&gross_floor.to_le_bytes());

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

    // Reload before every post-CPI balance read (FIX-06), then measure only
    // this swap's output rather than the sweep ATA's absolute balance.
    ctx.accounts.sweep_ata.reload()?;
    let gross = ctx
        .accounts
        .sweep_ata
        .amount
        .checked_sub(sweep_before)
        .ok_or(error!(TaxError::TaxOverflow))?;
    let tax_amount = calculate_tax(gross, tax_bps).ok_or(error!(TaxError::TaxOverflow))?;
    let net = gross
        .checked_sub(tax_amount)
        .ok_or(error!(TaxError::TaxOverflow))?;
    require!(net > 0, TaxError::InsufficientOutput);
    require!(net >= minimum_output, TaxError::SlippageExceeded);

    // Tax is already home in the sweep ATA. Forward exactly the net once; the
    // AMM never touches this dedicated destination account.
    let sweep_authority_seeds: &[&[u8]] = &[SWEEP_AUTHORITY_SEED, &[ctx.bumps.sweep_authority]];
    token_interface::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.quote_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.sweep_ata.to_account_info(),
                mint: ctx.accounts.quote_mint.to_account_info(),
                to: ctx.accounts.user_quote.to_account_info(),
                authority: ctx.accounts.sweep_authority.to_account_info(),
            },
            &[sweep_authority_seeds],
        ),
        net,
        ctx.accounts.quote_mint.decimals,
    )?;

    emit!(SplSwapExecuted {
        pool: ctx.accounts.pool.key(),
        quote_mint: view.quote_mint,
        is_crime: view.is_crime,
        direction,
        amount_in,
        tax_amount,
        amount_out: net,
        tax_rate_bps: tax_bps,
        epoch: epoch_state.current_epoch,
    });

    Ok(())
}

/// Positional accounts for the generic faction -> quote taxed lane.
///
/// The adapter/frontend MUST pass the canonical sweep ATA in the quote-side
/// `user_token_a` or `user_token_b` slot selected by pool orientation. The
/// faction-side slot is the user's signed input token account. `user_quote` is
/// a separate quote-mint destination used only by Tax for the final net
/// forward; the AMM never receives or touches it.
#[derive(Accounts)]
pub struct SwapSplSell<'info> {
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

    /// Pool-positional faction input or sweep-ATA quote output (see struct docs).
    /// CHECK: The handler binds the quote slot; the AMM token CPI validates mint and authority.
    #[account(mut)]
    pub user_token_a: AccountInfo<'info>,

    /// Pool-positional faction input or sweep-ATA quote output (see struct docs).
    /// CHECK: The handler binds the quote slot; the AMM token CPI validates mint and authority.
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

    /// Dedicated net-output destination. Its authority is intentionally not
    /// constrained; the token program validates only that its mint matches.
    #[account(
        mut,
        token::mint = quote_mint,
        token::token_program = quote_token_program,
    )]
    pub user_quote: InterfaceAccount<'info, TokenAccount>,

    pub quote_token_program: Interface<'info, TokenInterface>,

    /// CHECK: Forwarded positionally; the AMM validates it against mint A.
    pub token_program_a: AccountInfo<'info>,

    /// CHECK: Forwarded positionally; the AMM validates it against mint B.
    pub token_program_b: AccountInfo<'info>,

    /// CHECK: Canonical AMM address is enforced here.
    #[account(address = amm_program_id() @ TaxError::InvalidAmmProgram)]
    pub amm_program: AccountInfo<'info>,
}
