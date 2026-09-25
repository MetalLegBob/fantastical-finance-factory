use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{POOL_SEED, SWAP_AUTHORITY_SEED, TAX_PROGRAM_ID};
use crate::errors::AmmError;
use crate::events::SwapEvent;
use crate::helpers::math::{
    calculate_effective_input, calculate_swap_output, check_effective_input_nonzero,
    check_swap_output_nonzero, verify_k_invariant,
};
use crate::helpers::mint_policy::validate_amount_preserving_mint;
use crate::helpers::transfers::{transfer_spl, transfer_t22_checked};
use crate::state::pool::PoolState;

// ---------------------------------------------------------------------------
// SwapDirection enum
// ---------------------------------------------------------------------------

/// Direction of a swap through a pool.
///
/// Anchor serializes this as a single u8 variant index:
/// - 0 = AtoB (Token A in, Token B out)
/// - 1 = BtoA (Token B in, Token A out)
///
/// The caller explicitly declares direction. The AMM does not infer it from
/// account ordering (locked decision, see 11-CONTEXT.md).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum SwapDirection {
    /// Token A in, Token B out (variant 0).
    AtoB,
    /// Token B in, Token A out (variant 1).
    BtoA,
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Returns true if the given key is the Token-2022 program.
fn is_t22(key: &Pubkey) -> bool {
    *key == anchor_spl::token_2022::ID
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Execute a swap in a SOL pool (CRIME/SOL or FRAUD/SOL).
///
/// Follows strict CEI (Checks-Effects-Interactions) ordering:
/// 1. CHECKS: validate inputs, compute swap math, verify slippage
/// 2. EFFECTS: update pool reserves, verify k-invariant
/// 3. INTERACTIONS: execute token transfers
/// 4. POST-INTERACTION: clear reentrancy guard, emit event
///
/// LP fee is deducted before output calculation -- the fee stays in the pool
/// as part of the reserves, accruing value to LPs.
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SwapSolPool<'info>>,
    amount_in: u64,
    direction: SwapDirection,
    minimum_amount_out: u64,
) -> Result<()> {
    // Fail before taking the reentrancy lock or mutating reserves. Both sides
    // are checked on every operation so a newly scheduled fee cannot make
    // vault deltas diverge from PoolState's nominal accounting.
    validate_amount_preserving_mint(&ctx.accounts.mint_a.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;
    validate_amount_preserving_mint(&ctx.accounts.mint_b.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;

    // =========================================================================
    // Save immutable values from pool BEFORE any mutable access.
    //
    // Anchor's Account type uses RefCell internally. Once we mutate any field
    // via `&mut ctx.accounts.pool`, we cannot read other fields through a
    // separate immutable borrow. Capturing these upfront avoids borrow conflicts.
    // =========================================================================
    let mint_a_key = ctx.accounts.pool.mint_a;
    let mint_b_key = ctx.accounts.pool.mint_b;
    let pool_bump = ctx.accounts.pool.bump;
    let lp_fee_bps = ctx.accounts.pool.lp_fee_bps;
    let reserve_a = ctx.accounts.pool.reserve_a;
    let reserve_b = ctx.accounts.pool.reserve_b;
    let token_program_a_key = ctx.accounts.pool.token_program_a;
    let token_program_b_key = ctx.accounts.pool.token_program_b;

    // =====================================================================
    // CHECKS
    // =====================================================================

    // 1. Set reentrancy guard (Anchor constraint already verified !pool.locked)
    ctx.accounts.pool.locked = true;

    // 2. Validate non-zero input
    require!(amount_in > 0, AmmError::ZeroAmount);

    // 3. Direction-based account selection
    //    Bind reserves, decimals, and key references based on swap direction.
    //    Accounts are named by pool position (a/b), not role (input/output).
    let (
        reserve_in,
        reserve_out,
        input_decimals,
        output_decimals,
        input_mint_key,
        output_mint_key,
        input_tp_key,
        output_tp_key,
    ) = match direction {
        SwapDirection::AtoB => (
            reserve_a,
            reserve_b,
            ctx.accounts.mint_a.decimals,
            ctx.accounts.mint_b.decimals,
            mint_a_key,
            mint_b_key,
            token_program_a_key,
            token_program_b_key,
        ),
        SwapDirection::BtoA => (
            reserve_b,
            reserve_a,
            ctx.accounts.mint_b.decimals,
            ctx.accounts.mint_a.decimals,
            mint_b_key,
            mint_a_key,
            token_program_b_key,
            token_program_a_key,
        ),
    };

    // 4. Compute effective input after LP fee deduction
    let effective_input =
        calculate_effective_input(amount_in, lp_fee_bps).ok_or(AmmError::Overflow)?;

    // 4a. Zero-output check: fee must not consume entire input
    require!(
        check_effective_input_nonzero(amount_in, effective_input),
        AmmError::ZeroEffectiveInput
    );

    // 5. Compute swap output via constant-product formula
    let amount_out = calculate_swap_output(reserve_in, reserve_out, effective_input)
        .ok_or(AmmError::Overflow)?;

    // 5a. Zero-output check: swap must produce some output
    require!(
        check_swap_output_nonzero(effective_input, amount_out),
        AmmError::ZeroSwapOutput
    );

    // 6. Slippage protection
    require!(amount_out >= minimum_amount_out, AmmError::SlippageExceeded);

    // 7. Compute LP fee for event emission
    //    effective_input <= amount_in (both u64 range), so u64::try_from is safe.
    let effective_input_u64 =
        u64::try_from(effective_input).map_err(|_| error!(AmmError::Overflow))?;
    let lp_fee = amount_in
        .checked_sub(effective_input_u64)
        .ok_or(AmmError::Overflow)?;

    // =====================================================================
    // EFFECTS (reserve updates)
    // =====================================================================

    // 8. Compute new reserves (amount_in is pre-fee -- fee stays in pool)
    let new_reserve_in = reserve_in
        .checked_add(amount_in)
        .ok_or(AmmError::Overflow)?;
    let new_reserve_out = reserve_out
        .checked_sub(amount_out)
        .ok_or(AmmError::Overflow)?;

    // 9. Verify k-invariant: k_after >= k_before
    let k_valid = verify_k_invariant(reserve_in, reserve_out, new_reserve_in, new_reserve_out)
        .ok_or(AmmError::Overflow)?;
    require!(k_valid, AmmError::KInvariantViolation);

    // 10. Write new reserves to pool state
    match direction {
        SwapDirection::AtoB => {
            ctx.accounts.pool.reserve_a = new_reserve_in;
            ctx.accounts.pool.reserve_b = new_reserve_out;
        }
        SwapDirection::BtoA => {
            ctx.accounts.pool.reserve_b = new_reserve_in;
            ctx.accounts.pool.reserve_a = new_reserve_out;
        }
    }

    // =====================================================================
    // INTERACTIONS (token transfers)
    // =====================================================================

    // REMAINING_ACCOUNTS CONTRACT (VH-I001):
    // This function forwards ctx.remaining_accounts to BOTH transfer_t22_checked calls.
    // This works correctly because:
    //   - SOL pools: Only one side is Token-2022 (the token mint). The other side is
    //     native SOL (WSOL), which has no transfer hook. Extra accounts are ignored by
    //     the non-T22 transfer.
    //   - PROFIT pools (dual T22): The caller (Epoch Program's partition_hook_accounts)
    //     pre-splits [input_hooks, output_hooks] before CPI. The AMM receives pre-partitioned
    //     accounts and forwards them correctly.
    // If a new pool type is added with two different T22 mints that both have hooks,
    // the caller MUST partition remaining_accounts before CPI to this instruction.

    // 11. Build PDA signer seeds for vault-to-user transfers.
    //     Uses saved immutable values (mint keys, bump) from before mutations.
    let mint_a_bytes = mint_a_key.to_bytes();
    let mint_b_bytes = mint_b_key.to_bytes();
    let bump_bytes = [pool_bump];
    let pool_seeds: &[&[u8]] = &[POOL_SEED, &mint_a_bytes, &mint_b_bytes, &bump_bytes];
    let signer_seeds: &[&[&[u8]]] = &[pool_seeds];

    // Grab AccountInfo references for transfers.
    let pool_account_info = ctx.accounts.pool.to_account_info();
    let user_info = ctx.accounts.user.to_account_info();

    // Direction-aware transfer routing:
    // - Input transfer: user -> vault (user signs, no PDA signer)
    // - Output transfer: vault -> user (pool PDA signs)
    //
    // In mixed pools (CRIME/SOL, FRAUD/SOL), exactly one side is T22 and one
    // is SPL. Hook accounts (remaining_accounts) are only consumed by the T22
    // transfer. The SPL helper has no hook_accounts parameter.
    match direction {
        SwapDirection::AtoB => {
            // Input: A (user_token_a -> vault_a)
            if is_t22(&input_tp_key) {
                transfer_t22_checked(
                    &ctx.accounts.token_program_a.to_account_info(),
                    &ctx.accounts.user_token_a.to_account_info(),
                    &ctx.accounts.mint_a.to_account_info(),
                    &ctx.accounts.vault_a.to_account_info(),
                    &user_info,
                    amount_in,
                    input_decimals,
                    &[],
                    ctx.remaining_accounts,
                )?;
            } else {
                transfer_spl(
                    &ctx.accounts.token_program_a.to_account_info(),
                    &ctx.accounts.user_token_a.to_account_info(),
                    &ctx.accounts.mint_a.to_account_info(),
                    &ctx.accounts.vault_a.to_account_info(),
                    &user_info,
                    amount_in,
                    input_decimals,
                    &[],
                )?;
            }
            // Output: B (vault_b -> user_token_b)
            if is_t22(&output_tp_key) {
                transfer_t22_checked(
                    &ctx.accounts.token_program_b.to_account_info(),
                    &ctx.accounts.vault_b.to_account_info(),
                    &ctx.accounts.mint_b.to_account_info(),
                    &ctx.accounts.user_token_b.to_account_info(),
                    &pool_account_info,
                    amount_out,
                    output_decimals,
                    signer_seeds,
                    ctx.remaining_accounts,
                )?;
            } else {
                transfer_spl(
                    &ctx.accounts.token_program_b.to_account_info(),
                    &ctx.accounts.vault_b.to_account_info(),
                    &ctx.accounts.mint_b.to_account_info(),
                    &ctx.accounts.user_token_b.to_account_info(),
                    &pool_account_info,
                    amount_out,
                    output_decimals,
                    signer_seeds,
                )?;
            }
        }
        SwapDirection::BtoA => {
            // Input: B (user_token_b -> vault_b)
            if is_t22(&input_tp_key) {
                transfer_t22_checked(
                    &ctx.accounts.token_program_b.to_account_info(),
                    &ctx.accounts.user_token_b.to_account_info(),
                    &ctx.accounts.mint_b.to_account_info(),
                    &ctx.accounts.vault_b.to_account_info(),
                    &user_info,
                    amount_in,
                    input_decimals,
                    &[],
                    ctx.remaining_accounts,
                )?;
            } else {
                transfer_spl(
                    &ctx.accounts.token_program_b.to_account_info(),
                    &ctx.accounts.user_token_b.to_account_info(),
                    &ctx.accounts.mint_b.to_account_info(),
                    &ctx.accounts.vault_b.to_account_info(),
                    &user_info,
                    amount_in,
                    input_decimals,
                    &[],
                )?;
            }
            // Output: A (vault_a -> user_token_a)
            if is_t22(&output_tp_key) {
                transfer_t22_checked(
                    &ctx.accounts.token_program_a.to_account_info(),
                    &ctx.accounts.vault_a.to_account_info(),
                    &ctx.accounts.mint_a.to_account_info(),
                    &ctx.accounts.user_token_a.to_account_info(),
                    &pool_account_info,
                    amount_out,
                    output_decimals,
                    signer_seeds,
                    ctx.remaining_accounts,
                )?;
            } else {
                transfer_spl(
                    &ctx.accounts.token_program_a.to_account_info(),
                    &ctx.accounts.vault_a.to_account_info(),
                    &ctx.accounts.mint_a.to_account_info(),
                    &ctx.accounts.user_token_a.to_account_info(),
                    &pool_account_info,
                    amount_out,
                    output_decimals,
                    signer_seeds,
                )?;
            }
        }
    }

    // =====================================================================
    // POST-INTERACTION
    // =====================================================================

    // 12. Clear reentrancy guard
    ctx.accounts.pool.locked = false;

    // 13. Emit swap event
    let clock = Clock::get()?;
    emit!(SwapEvent {
        pool: ctx.accounts.pool.key(),
        user: ctx.accounts.user.key(),
        input_mint: input_mint_key,
        output_mint: output_mint_key,
        amount_in,
        amount_out,
        lp_fee,
        reserve_a: ctx.accounts.pool.reserve_a,
        reserve_b: ctx.accounts.pool.reserve_b,
        direction: direction as u8,
        timestamp: clock.unix_timestamp,
        slot: clock.slot,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Account struct
// ---------------------------------------------------------------------------

/// Accounts for the `swap_sol_pool` instruction.
///
/// Accounts are named by pool position (a/b), not by role (input/output).
/// This is because Anchor constraints are evaluated at deserialization time
/// (before the handler runs) and cannot branch on instruction arguments.
/// Direction-specific routing happens in the handler body.
///
/// Both user token accounts and both token programs are required regardless
/// of direction, because the handler determines input/output at runtime.
#[derive(Accounts)]
pub struct SwapSolPool<'info> {
    /// swap_authority PDA: must be signed by Tax Program via invoke_signed.
    ///
    /// The Signer type validates this account actually signed the transaction.
    /// The seeds + seeds::program constraint validates the PDA is derived
    /// from TAX_PROGRAM_ID with seeds ["swap_authority"].
    ///
    /// This ensures only the Tax Program can initiate swaps -- direct user
    /// calls without valid swap_authority will fail deserialization.
    #[account(
        seeds = [SWAP_AUTHORITY_SEED],
        bump,
        seeds::program = TAX_PROGRAM_ID,
    )]
    pub swap_authority: Signer<'info>,

    /// Pool state PDA. Mutable for reserve updates and reentrancy guard.
    /// Seeds validate this is the correct pool for the given mint pair.
    #[account(
        mut,
        seeds = [POOL_SEED, pool.mint_a.as_ref(), pool.mint_b.as_ref()],
        bump = pool.bump,
        constraint = pool.initialized @ AmmError::PoolNotInitialized,
        constraint = !pool.locked @ AmmError::PoolLocked,
    )]
    pub pool: Account<'info, PoolState>,

    /// Vault A: PDA-owned token account holding reserve A.
    /// Validated against pool state to prevent vault substitution attacks.
    #[account(
        mut,
        constraint = vault_a.key() == pool.vault_a @ AmmError::VaultMismatch,
    )]
    pub vault_a: InterfaceAccount<'info, TokenAccount>,

    /// Vault B: PDA-owned token account holding reserve B.
    #[account(
        mut,
        constraint = vault_b.key() == pool.vault_b @ AmmError::VaultMismatch,
    )]
    pub vault_b: InterfaceAccount<'info, TokenAccount>,

    /// Mint A: used for decimals in transfer_checked and token program routing.
    #[account(constraint = mint_a.key() == pool.mint_a @ AmmError::InvalidMint)]
    pub mint_a: InterfaceAccount<'info, Mint>,

    /// Mint B: used for decimals in transfer_checked and token program routing.
    #[account(constraint = mint_b.key() == pool.mint_b @ AmmError::InvalidMint)]
    pub mint_b: InterfaceAccount<'info, Mint>,

    /// User's token account for token A. Mutable for input or output transfers.
    /// No ownership constraint -- the token program validates authority during
    /// transfer_checked CPI (see 11-RESEARCH.md Open Question 3).
    #[account(mut)]
    pub user_token_a: InterfaceAccount<'info, TokenAccount>,

    /// User's token account for token B.
    #[account(mut)]
    pub user_token_b: InterfaceAccount<'info, TokenAccount>,

    /// The user executing the swap. Signs as authority for user-to-vault transfers.
    pub user: Signer<'info>,

    /// Token program for mint A (SPL Token or Token-2022).
    /// Validated against pool state to prevent program substitution.
    #[account(constraint = token_program_a.key() == pool.token_program_a @ AmmError::InvalidTokenProgram)]
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token program for mint B (SPL Token or Token-2022).
    #[account(constraint = token_program_b.key() == pool.token_program_b @ AmmError::InvalidTokenProgram)]
    pub token_program_b: Interface<'info, TokenInterface>,
}
