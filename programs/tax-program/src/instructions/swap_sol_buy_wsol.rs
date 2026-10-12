//! swap_sol_buy_wsol: SOL -> CRIME/FRAUD with buy tax paid from WSOL.
//!
//! Same economics as `swap_sol_buy`, but the WHOLE `amount_in` is debited from
//! the user's WSOL token account. Aggregators (Jupiter) hold route funds as
//! WSOL, so they cannot pay the tax from native lamports the way `swap_sol_buy`
//! requires.
//!
//! Tax flow reuses the sell lane's WSOL intermediary cycle:
//! 1. Tax WSOL: user_token_a -> wsol_intermediary (user signs)
//! 2. Close intermediary to swap_authority (unwraps WSOL to native SOL)
//! 3. Native SOL distributed from swap_authority: 71% staking, 24% carnage, 5% treasury
//! 4. Intermediary re-created and re-initialized for the next swap
//! 5. AMM swap_sol_pool CPI with the post-tax amount (user_token_a -> pool)
//!
//! CPI depth: Jupiter -> Tax -> AMM (2) -> Token-2022 (3) -> hook (4).

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
    rent::Rent,
    sysvar::Sysvar,
};
use anchor_lang::AccountDeserialize;
use anchor_spl::token::spl_token;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    amm_program_id, epoch_program_id, staking_program_id, treasury_pubkey, CARNAGE_SOL_VAULT_SEED,
    ESCROW_VAULT_SEED, STAKE_POOL_SEED, SWAP_AUTHORITY_SEED, TAX_AUTHORITY_SEED,
    WSOL_INTERMEDIARY_SEED,
};
use crate::errors::TaxError;
use crate::events::{PoolType, SwapDirection, TaxedSwap};
use crate::helpers::faction_pool_reader::read_faction_pool;
use crate::helpers::pool_reader::native_mint;
use crate::helpers::split::{execute_split_distribution, SplitDistributionAccounts};
use crate::helpers::tax_math::calculate_tax;
use crate::state::EpochState;

/// Execute a SOL -> CRIME/FRAUD swap with buy tax taken from WSOL.
///
/// Faction identity is derived from the authenticated pool, so there is no
/// `is_crime` witness argument.
///
/// # Arguments
/// * `amount_in` - Total WSOL the user spends (including tax)
/// * `minimum_output` - Minimum CRIME/FRAUD tokens expected (slippage protection)
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SwapSolBuyWsol<'info>>,
    amount_in: u64,
    minimum_output: u64,
) -> Result<()> {
    amm::helpers::mint_policy::validate_amount_preserving_mint(
        &ctx.accounts.mint_a.to_account_info(),
    )
    .map_err(|_| error!(TaxError::UnsupportedTransferFee))?;
    amm::helpers::mint_policy::validate_amount_preserving_mint(
        &ctx.accounts.mint_b.to_account_info(),
    )
    .map_err(|_| error!(TaxError::UnsupportedTransferFee))?;

    // The intermediary is a classic SPL Token account; the tax transfer, close
    // and re-init below must all go through SPL Token.
    require_keys_eq!(
        ctx.accounts.token_program_a.key(),
        spl_token::ID,
        TaxError::WsolProgramMismatch
    );

    // =========================================================================
    // 1. Read and validate EpochState
    // =========================================================================

    // Owner check: EpochState must be owned by Epoch Program.
    // CRITICAL: This prevents attackers from passing a fake EpochState with 0% tax.
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
    require!(
        Clock::get()?.slot >= epoch_state.pause_end_slot,
        TaxError::EpochPauseActive
    );

    // Bind the economic identity to the authenticated AMM pool before any
    // calculation or value movement. Fixed AtoB with WSOL in slot A, exactly
    // as swap_sol_buy.
    let pool_view = read_faction_pool(&ctx.accounts.pool)?;
    require!(!pool_view.faction_is_a, TaxError::NonWsolQuotePool);
    require_keys_eq!(
        pool_view.quote_mint,
        native_mint(),
        TaxError::NonWsolQuotePool
    );
    require_keys_eq!(
        ctx.accounts.mint_a.key(),
        pool_view.quote_mint,
        TaxError::PoolMintMismatch
    );
    let faction_mint = if pool_view.is_crime {
        crate::constants::crime_mint()
    } else {
        crate::constants::fraud_mint()
    };
    require_keys_eq!(
        ctx.accounts.mint_b.key(),
        faction_mint,
        TaxError::PoolMintMismatch
    );

    let derived_is_crime = pool_view.is_crime;
    let tax_bps = epoch_state.get_tax_bps(derived_is_crime, true);

    // =========================================================================
    // 2. Calculate tax and the post-tax swap amount
    // =========================================================================
    let tax_amount = calculate_tax(amount_in, tax_bps).ok_or(error!(TaxError::TaxOverflow))?;
    let sol_to_swap = amount_in
        .checked_sub(tax_amount)
        .ok_or(error!(TaxError::TaxOverflow))?;
    require!(sol_to_swap > 0, TaxError::InsufficientInput);

    // No protocol output floor: minimum_output is the caller's own slippage
    // bound, enforced by the AMM.

    let swap_authority_seeds: &[&[u8]] = &[SWAP_AUTHORITY_SEED, &[ctx.bumps.swap_authority]];

    // =========================================================================
    // 3. Transfer-Close-Distribute-Reinit: WSOL intermediary tax flow
    //    (identical sequence to swap_sol_sell step 8)
    // =========================================================================

    // --- 3a: Transfer tax WSOL from user to intermediary ---
    // User signed the top-level TX (or the caller's PDA signed the CPI).
    let transfer_tax_ix = Instruction {
        program_id: ctx.accounts.token_program_a.key(),
        accounts: vec![
            AccountMeta::new(ctx.accounts.user_token_a.key(), false),
            AccountMeta::new(ctx.accounts.wsol_intermediary.key(), false),
            AccountMeta::new_readonly(ctx.accounts.user.key(), true),
        ],
        data: {
            let mut d = vec![3u8]; // SPL Token Transfer instruction discriminator
            d.extend_from_slice(&tax_amount.to_le_bytes());
            d
        },
    };
    invoke(
        &transfer_tax_ix,
        &[
            ctx.accounts.user_token_a.to_account_info(),
            ctx.accounts.wsol_intermediary.to_account_info(),
            ctx.accounts.user.to_account_info(),
            ctx.accounts.token_program_a.to_account_info(),
        ],
    )?;

    // --- 3b: Close intermediary to swap_authority (unwrap WSOL) ---
    // close_account transfers ALL lamports (token balance + rent) to destination.
    let close_ix = Instruction {
        program_id: ctx.accounts.token_program_a.key(),
        accounts: vec![
            AccountMeta::new(ctx.accounts.wsol_intermediary.key(), false),
            AccountMeta::new(ctx.accounts.swap_authority.key(), false),
            AccountMeta::new_readonly(ctx.accounts.swap_authority.key(), true),
        ],
        data: vec![9u8], // CloseAccount instruction discriminator
    };
    invoke_signed(
        &close_ix,
        &[
            ctx.accounts.wsol_intermediary.to_account_info(),
            ctx.accounts.swap_authority.to_account_info(),
            ctx.accounts.token_program_a.to_account_info(),
        ],
        &[swap_authority_seeds],
    )?;

    // --- 3c: Distribute native SOL from swap_authority to 3 destinations ---
    // swap_authority now holds tax_amount + rent_exempt as native lamports.
    // Only the tax portions leave; rent lamports are retained for reinit.
    let (staking_portion, carnage_portion, treasury_portion) = execute_split_distribution(
        &SplitDistributionAccounts {
            source: &ctx.accounts.swap_authority,
            source_signer_seeds: Some(swap_authority_seeds),
            staking_escrow: &ctx.accounts.staking_escrow,
            stake_pool: &ctx.accounts.stake_pool,
            tax_authority: &ctx.accounts.tax_authority,
            tax_authority_bump: ctx.bumps.tax_authority,
            carnage_vault: &ctx.accounts.carnage_vault,
            treasury: &ctx.accounts.treasury,
            staking_program: &ctx.accounts.staking_program,
            system_program: &ctx.accounts.system_program.to_account_info(),
        },
        tax_amount,
    )?;

    // --- 3d: Recreate the intermediary at its PDA address ---
    let intermediary_seeds: &[&[u8]] = &[WSOL_INTERMEDIARY_SEED, &[ctx.bumps.wsol_intermediary]];
    let rent = Rent::get()?;
    let space = 165u64; // spl_token::state::Account::LEN
    let rent_lamports = rent.minimum_balance(space as usize);

    let create_ix = anchor_lang::solana_program::system_instruction::create_account(
        ctx.accounts.swap_authority.key,
        ctx.accounts.wsol_intermediary.key,
        rent_lamports,
        space,
        &ctx.accounts.token_program_a.key(),
    );
    invoke_signed(
        &create_ix,
        &[
            ctx.accounts.swap_authority.to_account_info(),
            ctx.accounts.wsol_intermediary.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
        &[swap_authority_seeds, intermediary_seeds],
    )?;

    // InitializeAccount3 (discriminator 18): owner = swap_authority, passed as data.
    let init_ix = Instruction {
        program_id: ctx.accounts.token_program_a.key(),
        accounts: vec![
            AccountMeta::new(ctx.accounts.wsol_intermediary.key(), false),
            AccountMeta::new_readonly(ctx.accounts.mint_a.key(), false),
        ],
        data: {
            let mut d = vec![18u8];
            d.extend_from_slice(&ctx.accounts.swap_authority.key().to_bytes());
            d
        },
    };
    invoke(
        &init_ix,
        &[
            ctx.accounts.wsol_intermediary.to_account_info(),
            ctx.accounts.mint_a.to_account_info(),
            ctx.accounts.token_program_a.to_account_info(),
        ],
    )?;

    // =========================================================================
    // 4. AMM CPI: swap sol_to_swap WSOL for CRIME/FRAUD (AtoB)
    //    Accounts in AMM SwapSolPool order; remaining_accounts forwarded for
    //    the Token-2022 transfer hook.
    // =========================================================================
    let mut account_metas = vec![
        AccountMeta::new_readonly(ctx.accounts.swap_authority.key(), true), // signer
        AccountMeta::new(ctx.accounts.pool.key(), false),
        AccountMeta::new(ctx.accounts.pool_vault_a.key(), false),
        AccountMeta::new(ctx.accounts.pool_vault_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_b.key(), false),
        AccountMeta::new(ctx.accounts.user_token_a.key(), false),
        AccountMeta::new(ctx.accounts.user_token_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.user.key(), true), // user also signs
        AccountMeta::new_readonly(ctx.accounts.token_program_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.token_program_b.key(), false),
    ];
    for account in ctx.remaining_accounts.iter() {
        if account.is_writable {
            account_metas.push(AccountMeta::new(account.key(), account.is_signer));
        } else {
            account_metas.push(AccountMeta::new_readonly(account.key(), account.is_signer));
        }
    }

    // sha256("global:swap_sol_pool")[0..8]; direction AtoB = 0.
    const AMM_SWAP_SOL_POOL_DISCRIMINATOR: [u8; 8] =
        [0xde, 0x80, 0x1e, 0x7b, 0x55, 0x27, 0x91, 0x8a];

    let mut ix_data = Vec::with_capacity(25);
    ix_data.extend_from_slice(&AMM_SWAP_SOL_POOL_DISCRIMINATOR);
    ix_data.extend_from_slice(&sol_to_swap.to_le_bytes());
    ix_data.push(0u8); // SwapDirection::AtoB = 0
    ix_data.extend_from_slice(&minimum_output.to_le_bytes());

    let ix = Instruction {
        program_id: ctx.accounts.amm_program.key(),
        accounts: account_metas,
        data: ix_data,
    };

    let mut account_infos = vec![
        ctx.accounts.swap_authority.to_account_info(),
        ctx.accounts.pool.to_account_info(),
        ctx.accounts.pool_vault_a.to_account_info(),
        ctx.accounts.pool_vault_b.to_account_info(),
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

    // Output measured by balance-diff (FIX-06 pattern).
    let token_b_before = ctx.accounts.user_token_b.amount;

    invoke_signed(&ix, &account_infos, &[swap_authority_seeds])?;

    ctx.accounts.user_token_b.reload()?;
    let tokens_received = ctx
        .accounts
        .user_token_b
        .amount
        .checked_sub(token_b_before)
        .ok_or(error!(TaxError::TaxOverflow))?;

    // =========================================================================
    // 5. Emit TaxedSwap event (same shape as swap_sol_buy)
    // =========================================================================
    let clock = Clock::get()?;

    emit!(TaxedSwap {
        user: ctx.accounts.user.key(),
        pool_type: if derived_is_crime {
            PoolType::SolCrime
        } else {
            PoolType::SolFraud
        },
        direction: SwapDirection::Buy,
        input_amount: amount_in,
        output_amount: tokens_received,
        tax_amount,
        tax_rate_bps: tax_bps,
        staking_portion,
        carnage_portion,
        treasury_portion,
        epoch: epoch_state.current_epoch,
        slot: clock.slot,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Account struct
// ---------------------------------------------------------------------------

/// Accounts for swap_sol_buy_wsol (SOL -> CRIME/FRAUD, tax paid from WSOL).
///
/// Same accounts and order as `SwapSolSell` (the sell lane already carries the
/// WSOL intermediary), so integrators reuse one account layout for both
/// directions. Direction is AtoB (Token A = WSOL in, Token B = CRIME/FRAUD out).
#[derive(Accounts)]
pub struct SwapSolBuyWsol<'info> {
    /// User initiating the swap - authority over user_token_a.
    /// Not mutable: this lane never debits the user's native lamports.
    pub user: Signer<'info>,

    /// EpochState account from Epoch Program.
    ///
    /// CHECK: Validated manually in handler:
    /// - Owner check: must be Epoch Program (prevents fake 0% tax)
    /// - Deserialization validates discriminator
    /// - initialized flag checked
    pub epoch_state: AccountInfo<'info>,

    /// Tax Program's swap_authority PDA - signs AMM CPI and tax distribution.
    /// Mutable because it receives lamports from close_account (unwrap)
    /// and sends them to tax destinations via system transfers.
    /// CHECK: PDA derived from seeds, used as signer for CPI
    #[account(
        mut,
        seeds = [SWAP_AUTHORITY_SEED],
        bump,
    )]
    pub swap_authority: AccountInfo<'info>,

    /// Tax Program's tax_authority PDA - signs Staking Program CPI
    /// CHECK: PDA derived from seeds, used as signer for deposit_rewards CPI
    #[account(
        seeds = [TAX_AUTHORITY_SEED],
        bump,
    )]
    pub tax_authority: AccountInfo<'info>,

    /// AMM pool state - mutable for reserve updates
    /// CHECK: Owner + layout validated by read_faction_pool; re-validated in AMM CPI
    #[account(mut)]
    pub pool: AccountInfo<'info>,

    /// Pool's WSOL vault (Token A)
    #[account(mut)]
    pub pool_vault_a: InterfaceAccount<'info, TokenAccount>,

    /// Pool's CRIME/FRAUD vault (Token B)
    #[account(mut)]
    pub pool_vault_b: InterfaceAccount<'info, TokenAccount>,

    /// WSOL mint
    pub mint_a: InterfaceAccount<'info, Mint>,

    /// CRIME or FRAUD mint (Token-2022)
    pub mint_b: InterfaceAccount<'info, Mint>,

    /// User's WSOL token account - pays the full amount_in (tax + swap)
    #[account(mut)]
    pub user_token_a: InterfaceAccount<'info, TokenAccount>,

    /// User's CRIME/FRAUD token account - receives AMM output
    #[account(mut)]
    pub user_token_b: InterfaceAccount<'info, TokenAccount>,

    /// Staking Program's StakePool PDA - updated by deposit_rewards CPI
    /// CHECK: PDA validated by Staking Program via seeds constraint
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump,
        seeds::program = staking_program_id(),
    )]
    pub stake_pool: AccountInfo<'info>,

    /// Staking Program escrow - receives 71% of tax (native SOL)
    /// CHECK: PDA derived from Staking Program seeds
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED],
        bump,
        seeds::program = staking_program_id(),
        constraint = true @ TaxError::InvalidStakingEscrow,
    )]
    pub staking_escrow: AccountInfo<'info>,

    /// Carnage Fund vault - receives 24% of tax (native SOL)
    /// CHECK: PDA derived from Epoch Program seeds
    #[account(
        mut,
        seeds = [CARNAGE_SOL_VAULT_SEED],
        bump,
        seeds::program = epoch_program_id(),
        constraint = true @ TaxError::InvalidCarnageVault,
    )]
    pub carnage_vault: AccountInfo<'info>,

    /// Protocol treasury - receives 5% of tax (native SOL)
    /// CHECK: Address validated against known treasury pubkey
    #[account(
        mut,
        address = treasury_pubkey() @ TaxError::InvalidTreasury,
    )]
    pub treasury: AccountInfo<'info>,

    /// Protocol-owned WSOL intermediary (same PDA the sell lane uses).
    /// Closed and re-created each swap to convert tax WSOL -> native SOL.
    ///
    /// CHECK: PDA derived from known seeds. Created/closed within handler.
    #[account(
        mut,
        seeds = [WSOL_INTERMEDIARY_SEED],
        bump,
    )]
    pub wsol_intermediary: AccountInfo<'info>,

    /// AMM Program for swap CPI
    /// CHECK: Address validated against known AMM program ID
    #[account(address = amm_program_id() @ TaxError::InvalidAmmProgram)]
    pub amm_program: AccountInfo<'info>,

    /// SPL Token program (for WSOL) - pinned to SPL Token in the handler
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token-2022 program (for CRIME/FRAUD)
    pub token_program_b: Interface<'info, TokenInterface>,

    /// System program (for native SOL transfers and account creation)
    pub system_program: Program<'info, System>,

    /// Staking Program for deposit_rewards CPI
    /// CHECK: Program ID validated in constants.rs staking_program_id()
    #[account(address = staking_program_id() @ TaxError::InvalidStakingProgram)]
    pub staking_program: AccountInfo<'info>,
}
