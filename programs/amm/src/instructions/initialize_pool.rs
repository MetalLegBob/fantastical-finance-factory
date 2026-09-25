use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    ADMIN_SEED, MAX_LP_FEE_BPS, POOL_SEED, VAULT_A_SEED, VAULT_B_SEED, VAULT_SEED,
};
use crate::errors::AmmError;
use crate::events::PoolInitializedEvent;
use crate::helpers::mint_policy::validate_amount_preserving_mint;
use crate::helpers::transfers::{transfer_spl, transfer_t22_checked};
use crate::state::{AdminConfig, PoolState, PoolType};

/// Creates a new AMM pool with PDA-owned vaults and seeds initial liquidity.
///
/// This instruction performs all pool setup atomically in a single transaction:
/// 1. Validates canonical mint ordering (mint_a < mint_b)
/// 2. Validates non-duplicate mints and non-zero seed amounts
/// 3. Infers pool type from token programs (MixedPool vs PureT22Pool)
/// 4. Initializes the pool state PDA
/// 5. Creates PDA-owned vault token accounts (pool PDA as authority)
/// 6. Transfers initial liquidity from admin's token accounts into vaults
/// 7. Emits PoolInitializedEvent
///
/// # Arguments
/// * `lp_fee_bps` - LP fee in basis points (e.g., 100 = 1%)
/// * `amount_a` - Initial seed amount for token A (must be > 0)
/// * `amount_b` - Initial seed amount for token B (must be > 0)
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, InitializePool<'info>>,
    lp_fee_bps: u16,
    amount_a: u64,
    amount_b: u64,
) -> Result<()> {
    // --- Validation ---

    // Canonical ordering: mint_a must be the "smaller" pubkey.
    // This guarantees exactly one PDA per unordered mint pair.
    require!(
        ctx.accounts.mint_a.key() < ctx.accounts.mint_b.key(),
        AmmError::MintsNotCanonicallyOrdered
    );

    // Duplicate mints would create a nonsensical self-pairing pool.
    require!(
        ctx.accounts.mint_a.key() != ctx.accounts.mint_b.key(),
        AmmError::DuplicateMints
    );

    // Zero seed amounts would cause division-by-zero in the CPMM formula
    // (reserve_a * reserve_b = k, and swap math divides by reserves).
    require!(amount_a > 0 && amount_b > 0, AmmError::ZeroSeedAmount);

    // Reserve accounting assumes every nominal transfer amount is credited in
    // full. Swaps and liquidity operations repeat this validation because a
    // fee-config authority may schedule a fee after pool initialization.
    validate_amount_preserving_mint(&ctx.accounts.mint_a.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;
    validate_amount_preserving_mint(&ctx.accounts.mint_b.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;

    // --- Pool type inference ---
    // Inferred from token programs, NOT caller-declared.
    // This prevents misclassification attacks where a caller could
    // declare PureT22Pool for a mixed pair to skip transfer hooks.
    let pool_type = infer_pool_type(
        ctx.accounts.token_program_a.key,
        ctx.accounts.token_program_b.key,
    )?;

    // --- Transfer initial liquidity into vaults ---
    // Both transfers happen atomically in this instruction.
    // If either fails, the entire transaction reverts.
    //
    // For Token-2022 mints with Transfer Hook extensions, we use the hook-aware
    // transfer helpers that forward remaining_accounts. Anchor's built-in
    // transfer_checked does NOT forward remaining_accounts, which causes
    // Token-2022 to fail when invoking the hook.
    //
    // remaining_accounts contain ExtraAccountMetas for hook resolution.

    let tp_a_key = ctx.accounts.token_program_a.key();
    let tp_b_key = ctx.accounts.token_program_b.key();

    // Transfer token A from admin's source account into vault A.
    if tp_a_key == anchor_spl::token_2022::ID {
        transfer_t22_checked(
            &ctx.accounts.token_program_a.to_account_info(),
            &ctx.accounts.source_a.to_account_info(),
            &ctx.accounts.mint_a.to_account_info(),
            &ctx.accounts.vault_a.to_account_info(),
            &ctx.accounts.admin.to_account_info(),
            amount_a,
            ctx.accounts.mint_a.decimals,
            &[], // admin signs, not a PDA
            ctx.remaining_accounts,
        )?;
    } else {
        transfer_spl(
            &ctx.accounts.token_program_a.to_account_info(),
            &ctx.accounts.source_a.to_account_info(),
            &ctx.accounts.mint_a.to_account_info(),
            &ctx.accounts.vault_a.to_account_info(),
            &ctx.accounts.admin.to_account_info(),
            amount_a,
            ctx.accounts.mint_a.decimals,
            &[], // admin signs, not a PDA
        )?;
    }

    // Transfer token B from admin's source account into vault B.
    if tp_b_key == anchor_spl::token_2022::ID {
        transfer_t22_checked(
            &ctx.accounts.token_program_b.to_account_info(),
            &ctx.accounts.source_b.to_account_info(),
            &ctx.accounts.mint_b.to_account_info(),
            &ctx.accounts.vault_b.to_account_info(),
            &ctx.accounts.admin.to_account_info(),
            amount_b,
            ctx.accounts.mint_b.decimals,
            &[], // admin signs, not a PDA
            ctx.remaining_accounts,
        )?;
    } else {
        transfer_spl(
            &ctx.accounts.token_program_b.to_account_info(),
            &ctx.accounts.source_b.to_account_info(),
            &ctx.accounts.mint_b.to_account_info(),
            &ctx.accounts.vault_b.to_account_info(),
            &ctx.accounts.admin.to_account_info(),
            amount_b,
            ctx.accounts.mint_b.decimals,
            &[], // admin signs, not a PDA
        )?;
    }

    // --- Populate pool state ---
    let pool = &mut ctx.accounts.pool;
    pool.pool_type = pool_type;
    pool.mint_a = ctx.accounts.mint_a.key();
    pool.mint_b = ctx.accounts.mint_b.key();
    pool.vault_a = ctx.accounts.vault_a.key();
    pool.vault_b = ctx.accounts.vault_b.key();
    pool.reserve_a = amount_a;
    pool.reserve_b = amount_b;
    require!(lp_fee_bps <= MAX_LP_FEE_BPS, AmmError::LpFeeExceedsMax);
    pool.lp_fee_bps = lp_fee_bps;
    pool.initialized = true;
    pool.bump = ctx.bumps.pool;
    pool.vault_a_bump = ctx.bumps.vault_a;
    pool.vault_b_bump = ctx.bumps.vault_b;
    pool.token_program_a = ctx.accounts.token_program_a.key();
    pool.token_program_b = ctx.accounts.token_program_b.key();

    // --- Emit event ---
    let pool_type_u8 = match pool_type {
        PoolType::MixedPool => 0u8,
        PoolType::PureT22Pool => 1u8,
    };

    emit!(PoolInitializedEvent {
        pool: pool.key(),
        pool_type: pool_type_u8,
        mint_a: pool.mint_a,
        mint_b: pool.mint_b,
        vault_a: pool.vault_a,
        vault_b: pool.vault_b,
        reserve_a: pool.reserve_a,
        reserve_b: pool.reserve_b,
        lp_fee_bps: pool.lp_fee_bps,
    });

    msg!(
        "Pool initialized: {} / {} (type {})",
        pool.mint_a,
        pool.mint_b,
        pool_type_u8
    );

    Ok(())
}

/// Infer pool type from the token programs of each mint.
///
/// - If both are Token-2022: PureT22Pool
/// - If one is SPL Token and one is Token-2022: MixedPool
/// - If both are SPL Token: MixedPool (treated as legacy pair)
/// - If either is an unrecognized program: InvalidTokenProgram error
fn infer_pool_type(token_program_a: &Pubkey, token_program_b: &Pubkey) -> Result<PoolType> {
    let a_is_t22 = *token_program_a == anchor_spl::token_2022::ID;
    let b_is_t22 = *token_program_b == anchor_spl::token_2022::ID;
    let a_is_spl = *token_program_a == anchor_spl::token::ID;
    let b_is_spl = *token_program_b == anchor_spl::token::ID;

    // Both must be recognized token programs.
    require!(
        (a_is_t22 || a_is_spl) && (b_is_t22 || b_is_spl),
        AmmError::InvalidTokenProgram
    );

    if a_is_t22 && b_is_t22 {
        Ok(PoolType::PureT22Pool)
    } else {
        Ok(PoolType::MixedPool)
    }
}

#[derive(Accounts)]
#[instruction(lp_fee_bps: u16, amount_a: u64, amount_b: u64)]
pub struct InitializePool<'info> {
    /// The payer for account rent. Typically the admin, but can be a separate funder.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The global AdminConfig PDA. Verified via has_one = admin constraint
    /// to ensure only the authorized admin can create pools.
    #[account(
        seeds = [ADMIN_SEED],
        bump = admin_config.bump,
        has_one = admin @ AmmError::Unauthorized,
    )]
    pub admin_config: Account<'info, AdminConfig>,

    /// The admin signer. Must match admin_config.admin.
    /// Also acts as the authority for the initial liquidity transfers.
    pub admin: Signer<'info>,

    /// The pool state PDA. Derived from canonical mint pair.
    /// Seeds: [b"pool", mint_a.key().as_ref(), mint_b.key().as_ref()]
    #[account(
        init,
        payer = payer,
        space = 8 + PoolState::INIT_SPACE,
        seeds = [POOL_SEED, mint_a.key().as_ref(), mint_b.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, PoolState>,

    /// Vault A: PDA-owned token account for reserve A.
    /// The pool PDA is the authority, ensuring only the program can move funds.
    /// Seeds: [b"vault", pool.key().as_ref(), b"a"]
    #[account(
        init,
        payer = payer,
        token::mint = mint_a,
        token::authority = pool,
        token::token_program = token_program_a,
        seeds = [VAULT_SEED, pool.key().as_ref(), VAULT_A_SEED],
        bump
    )]
    pub vault_a: InterfaceAccount<'info, TokenAccount>,

    /// Vault B: PDA-owned token account for reserve B.
    /// Seeds: [b"vault", pool.key().as_ref(), b"b"]
    #[account(
        init,
        payer = payer,
        token::mint = mint_b,
        token::authority = pool,
        token::token_program = token_program_b,
        seeds = [VAULT_SEED, pool.key().as_ref(), VAULT_B_SEED],
        bump
    )]
    pub vault_b: InterfaceAccount<'info, TokenAccount>,

    /// Mint A (the canonically smaller pubkey).
    /// Constraint: on-chain owner must match the provided token_program_a.
    /// This prevents passing a T22 mint with the SPL Token program (or vice versa).
    #[account(
        constraint = mint_a.to_account_info().owner == token_program_a.key @ AmmError::InvalidTokenProgram
    )]
    pub mint_a: InterfaceAccount<'info, Mint>,

    /// Mint B (the canonically larger pubkey).
    /// Same owner validation as mint_a.
    #[account(
        constraint = mint_b.to_account_info().owner == token_program_b.key @ AmmError::InvalidTokenProgram
    )]
    pub mint_b: InterfaceAccount<'info, Mint>,

    /// Admin's source token account for mint A.
    /// Must have sufficient balance for the initial seed amount.
    #[account(mut)]
    pub source_a: InterfaceAccount<'info, TokenAccount>,

    /// Admin's source token account for mint B.
    #[account(mut)]
    pub source_b: InterfaceAccount<'info, TokenAccount>,

    /// Token program for mint A (SPL Token or Token-2022).
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token program for mint B (SPL Token or Token-2022).
    pub token_program_b: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}
