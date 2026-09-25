use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{epoch_program_id, ARB_CONFIG_SEED, POOL_SEED};
use crate::errors::AmmError;
use crate::events::LiquidityRemoved;
use crate::helpers::mint_policy::validate_amount_preserving_mint;
use crate::helpers::transfers::{transfer_spl, transfer_t22_checked};
use crate::state::pool::PoolState;
use crate::state::ArbConfig;

// ---------------------------------------------------------------------------
// remove_liquidity_double_sided -- Squads-governed protocol withdrawal
// ---------------------------------------------------------------------------
//
// Protocol-owned, reserve-decrementing liquidity removal for any AMM pool.
//
// Design invariants (locked in spec §14.8):
//
// 1. SQUADS-AUTHORITY-GATED: the signer must equal the authority stored in the
//    Epoch Program's canonical ArbConfig PDA. The PDA address, owner, Anchor
//    discriminator, and authority field are all validated before any effect.
//
// 2. PERCENTAGE FORM: Squads approves `share_bps`; both payout amounts are
//    computed from the pool's live reserves at execution. No caller-provided
//    token amount can go stale during the governance timelock.
//
// 3. PRICE-NEUTRAL BY CONSTRUCTION: both reserve sides are scaled by the same
//    share. Their ratio is preserved up to the independent floor rounding.
//    A universal 1,000-raw-unit post-removal floor preserves the price anchor.
//
// 4. ORIENTATION-AGNOSTIC: sides A and B are handled symmetrically. Removal
//    never assumes which stored side contains a faction or quote mint.
//
// 5. NO LP MINT: pools have no LP token. This Squads-gated instruction is the
//    only path by which protocol-owned reserves leave their vaults.
//
// 6. UNIFORM RE-ENTRANCY DISCIPLINE: the same `!pool.locked` constraint and
//    lock/unlock pattern as add and swap protects both transfer CPI windows.
//
// 7. WSOL STAYS WRAPPED: a native-mint payout lands in the destination WSOL
//    ATA. This instruction never unwraps, closes, or syncs native accounts.

/// Returns true if the given key is the Token-2022 program.
fn is_t22(key: &Pubkey) -> bool {
    *key == anchor_spl::token_2022::ID
}

/// Compute both payout amounts from the live reserves and approved share.
///
/// This is the complete remove-math contract mirrored by the Phase-172 bot
/// and proved by the Phase-169 Kani harnesses. Each amount is floored:
///
/// ```text
/// amount_x = floor(reserve_x * share_bps / 10_000)
/// ```
///
/// A successful result is always two-sided and leaves at least 1,000 raw
/// units on each side. Dust breaches reject the proposal; amounts are never
/// clamped away from the exact percentage Squads reviewed.
pub fn compute_remove_amounts(
    reserve_a: u64,
    reserve_b: u64,
    share_bps: u16,
) -> Result<(u64, u64)> {
    require!((1..=9_999).contains(&share_bps), AmmError::InvalidShare);

    let amount_a = u64::try_from(
        (reserve_a as u128)
            .checked_mul(share_bps as u128)
            .ok_or(AmmError::Overflow)?
            .checked_div(10_000)
            .ok_or(AmmError::Overflow)?,
    )
    .map_err(|_| AmmError::Overflow)?;
    let amount_b = u64::try_from(
        (reserve_b as u128)
            .checked_mul(share_bps as u128)
            .ok_or(AmmError::Overflow)?
            .checked_div(10_000)
            .ok_or(AmmError::Overflow)?,
    )
    .map_err(|_| AmmError::Overflow)?;

    require!(amount_a > 0 && amount_b > 0, AmmError::ZeroLiquidity);

    let remaining_a = reserve_a.checked_sub(amount_a).ok_or(AmmError::Overflow)?;
    let remaining_b = reserve_b.checked_sub(amount_b).ok_or(AmmError::Overflow)?;
    require!(remaining_a >= 1_000, AmmError::DustFloorBreached);
    require!(remaining_b >= 1_000, AmmError::DustFloorBreached);

    Ok((amount_a, amount_b))
}

/// Remove a percentage of both live reserves and pay them to one allowlisted
/// destination owner's canonical token accounts.
///
/// Strict CEI order:
/// 1. CHECKS: lock, validate share, ArbConfig authority, destination, and math
/// 2. EFFECTS: decrement both reserves
/// 3. INTERACTIONS: transfer both vault payouts with the pool PDA signer
/// 4. POST-INTERACTION: clear the lock and emit the four-field event
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, RemoveLiquidityDoubleSided<'info>>,
    share_bps: u16,
) -> Result<()> {
    // =====================================================================
    // CHECKS
    // =====================================================================

    // Transfer fees would under-credit the governed destination relative to
    // the approved percentage. Validate before the first state effect.
    validate_amount_preserving_mint(&ctx.accounts.mint_a.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;
    validate_amount_preserving_mint(&ctx.accounts.mint_b.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;

    // 1. Lock FIRST. The account constraint established that the incoming
    //    state was unlocked; any later failure rolls this write back.
    ctx.accounts.pool.locked = true;

    // InvalidShare is the first semantic guard. The pure contract repeats
    // this check below so direct proof/bot consumers observe identical rules.
    require!((1..=9_999).contains(&share_bps), AmmError::InvalidShare);

    // Capture immutable pool values before further mutable access.
    let pool_key = ctx.accounts.pool.key();
    let mint_a_key = ctx.accounts.pool.mint_a;
    let mint_b_key = ctx.accounts.pool.mint_b;
    let reserve_a = ctx.accounts.pool.reserve_a;
    let reserve_b = ctx.accounts.pool.reserve_b;
    let token_program_a_key = ctx.accounts.pool.token_program_a;
    let token_program_b_key = ctx.accounts.pool.token_program_b;
    let pool_bump = ctx.accounts.pool.bump;

    // Read and release the canonical ArbConfig before either transfer CPI.
    require!(
        ctx.accounts.arb_config.owner == &epoch_program_id(),
        AmmError::InvalidArbConfig
    );
    let (config_authority, wallet_a, wallet_b) = {
        let data = ctx.accounts.arb_config.try_borrow_data()?;
        let cfg = ArbConfig::try_deserialize(&mut &data[..])
            .map_err(|_| error!(AmmError::InvalidArbConfig))?;
        (cfg.authority, cfg.wallet_a, cfg.wallet_b)
    };

    let authority_key = ctx.accounts.authority.key();
    require!(authority_key == config_authority, AmmError::Unauthorized);

    let destination = ctx.accounts.destination_owner.key();
    require!(
        destination == wallet_a || destination == wallet_b || destination == config_authority,
        AmmError::InvalidDestination
    );

    // Computes both floor-rounded sides, rejects zero payouts, and enforces
    // the universal post-removal dust floor before any reserve effect.
    let (amount_a, amount_b) = compute_remove_amounts(reserve_a, reserve_b, share_bps)?;

    // =====================================================================
    // EFFECTS (reserve decrements BEFORE transfer CPIs -- strict CEI)
    // =====================================================================

    // Conservative remove direction: during the interaction window the pool
    // claims LESS than its vaults still hold, never more.
    ctx.accounts.pool.reserve_a = reserve_a.checked_sub(amount_a).ok_or(AmmError::Overflow)?;
    ctx.accounts.pool.reserve_b = reserve_b.checked_sub(amount_b).ok_or(AmmError::Overflow)?;

    // Anchor normally serializes Account<T> at instruction exit. Persist the
    // CEI effects before either transfer so hook-time observers see the live
    // lock and conservative reserves. Transaction rollback restores the
    // original bytes if any later interaction fails.
    ctx.accounts.pool.exit(ctx.program_id)?;

    // =====================================================================
    // INTERACTIONS (vaults -> destination ATAs, pool PDA signs)
    // =====================================================================

    // Verbatim signer derivation used by swap_sol_pool vault payouts.
    let mint_a_bytes = mint_a_key.to_bytes();
    let mint_b_bytes = mint_b_key.to_bytes();
    let bump_bytes = [pool_bump];
    let pool_seeds: &[&[u8]] = &[POOL_SEED, &mint_a_bytes, &mint_b_bytes, &bump_bytes];
    let signer_seeds: &[&[&[u8]]] = &[pool_seeds];

    let pool_account_info = ctx.accounts.pool.to_account_info();

    // Both sides are routed solely by the token programs stored in PoolState.
    // A hooked T22 leg receives the caller-supplied hook account group and the
    // pool signer seeds; its source vault is the allowlisted hook endpoint.
    if is_t22(&token_program_a_key) {
        transfer_t22_checked(
            &ctx.accounts.token_program_a.to_account_info(),
            &ctx.accounts.vault_a.to_account_info(),
            &ctx.accounts.mint_a.to_account_info(),
            &ctx.accounts.destination_a.to_account_info(),
            &pool_account_info,
            amount_a,
            ctx.accounts.mint_a.decimals,
            signer_seeds,
            ctx.remaining_accounts,
        )?;
    } else {
        transfer_spl(
            &ctx.accounts.token_program_a.to_account_info(),
            &ctx.accounts.vault_a.to_account_info(),
            &ctx.accounts.mint_a.to_account_info(),
            &ctx.accounts.destination_a.to_account_info(),
            &pool_account_info,
            amount_a,
            ctx.accounts.mint_a.decimals,
            signer_seeds,
        )?;
    }

    if is_t22(&token_program_b_key) {
        transfer_t22_checked(
            &ctx.accounts.token_program_b.to_account_info(),
            &ctx.accounts.vault_b.to_account_info(),
            &ctx.accounts.mint_b.to_account_info(),
            &ctx.accounts.destination_b.to_account_info(),
            &pool_account_info,
            amount_b,
            ctx.accounts.mint_b.decimals,
            signer_seeds,
            ctx.remaining_accounts,
        )?;
    } else {
        transfer_spl(
            &ctx.accounts.token_program_b.to_account_info(),
            &ctx.accounts.vault_b.to_account_info(),
            &ctx.accounts.mint_b.to_account_info(),
            &ctx.accounts.destination_b.to_account_info(),
            &pool_account_info,
            amount_b,
            ctx.accounts.mint_b.decimals,
            signer_seeds,
        )?;
    }

    // =====================================================================
    // POST-INTERACTION
    // =====================================================================

    ctx.accounts.pool.locked = false;
    emit!(LiquidityRemoved {
        pool: pool_key,
        amount_a,
        amount_b,
        destination,
    });

    Ok(())
}

/// Accounts for the only protocol-owned-liquidity withdrawal path.
///
/// Declaration order is load-bearing: Anchor validates fields top-down, so
/// the Squads signer and canonical ArbConfig are the first gate accounts.
#[derive(Accounts)]
pub struct RemoveLiquidityDoubleSided<'info> {
    /// Squads vault PDA signer and rent payer for missing destination ATAs.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// Epoch Program's canonical ArbConfig PDA.
    /// CHECK: owner + discriminator + field read validated in handler
    /// (GAP-1 idiom, spec §14.2).
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub arb_config: AccountInfo<'info>,

    /// Pool state PDA, mutable for reserve decrements and the lock.
    #[account(
        mut,
        seeds = [POOL_SEED, pool.mint_a.as_ref(), pool.mint_b.as_ref()],
        bump = pool.bump,
        constraint = pool.initialized @ AmmError::PoolNotInitialized,
        constraint = !pool.locked @ AmmError::PoolLocked,
    )]
    pub pool: Account<'info, PoolState>,

    #[account(
        mut,
        constraint = vault_a.key() == pool.vault_a @ AmmError::VaultMismatch,
    )]
    pub vault_a: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = vault_b.key() == pool.vault_b @ AmmError::VaultMismatch,
    )]
    pub vault_b: InterfaceAccount<'info, TokenAccount>,

    #[account(constraint = mint_a.key() == pool.mint_a @ AmmError::InvalidMint)]
    pub mint_a: InterfaceAccount<'info, Mint>,

    #[account(constraint = mint_b.key() == pool.mint_b @ AmmError::InvalidMint)]
    pub mint_b: InterfaceAccount<'info, Mint>,

    /// Destination owner; validity comes ONLY from the handler allowlist
    /// (spec §14.8 entry 1).
    /// CHECK: compared with ArbConfig wallet A, wallet B, and authority.
    pub destination_owner: AccountInfo<'info>,

    /// Canonical ATA for destination owner and mint A.
    ///
    /// Rent-grief design note: Anchor evaluates `init_if_needed` before the
    /// handler allowlist. If that handler check fails, Solana rolls back the
    /// entire transaction, including this ATA creation; rejection creates
    /// nothing and spends no rent.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = mint_a,
        associated_token::authority = destination_owner,
        associated_token::token_program = token_program_a,
    )]
    pub destination_a: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Canonical ATA for destination owner and mint B. The same transaction
    /// rollback guarantee applies if the allowlist rejects the owner.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = mint_b,
        associated_token::authority = destination_owner,
        associated_token::token_program = token_program_b,
    )]
    pub destination_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(constraint = token_program_a.key() == pool.token_program_a @ AmmError::InvalidTokenProgram)]
    pub token_program_a: Interface<'info, TokenInterface>,

    #[account(constraint = token_program_b.key() == pool.token_program_b @ AmmError::InvalidTokenProgram)]
    pub token_program_b: Interface<'info, TokenInterface>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}
