use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{crime_mint, epoch_program_id, fraud_mint, ARB_CONFIG_SEED, POOL_SEED};
use crate::errors::AmmError;
use crate::events::LiquidityAddedEvent;
use crate::helpers::mint_policy::validate_amount_preserving_mint;
use crate::helpers::transfers::{transfer_spl, transfer_t22_checked};
use crate::state::pool::PoolState;
use crate::state::ArbConfig;

// ---------------------------------------------------------------------------
// add_liquidity_double_sided -- the Policy-B DEEPEN deposit (VOTE-07 core)
// ---------------------------------------------------------------------------
//
// Protocol-owned, reserve-incrementing liquidity add for any pool containing
// exactly one faction mint (CRIME or FRAUD).
//
// Design invariants (locked in 162-CONTEXT):
//
// 1. PROTOCOL-ONLY: the signer must be one of the two wallets stored in the
//    Epoch Program's canonical ArbConfig PDA. The PDA address, owner, Anchor
//    discriminator, and wallet field are all validated before any effect.
//
// 2. NO LP MINT: nothing is minted anywhere. The pools have no LP mint --
//    "adding liquidity" means transferring into the vaults and incrementing
//    `reserve_a`/`reserve_b`. Depth added this way is permanently owned by
//    the protocol (the north-star: pool depth IS the backing).
//
// 3. FACTION-SIDE-EXACT: the caller passes `faction_amount` (its ENTIRE
//    just-bought faction balance) plus `max_quote` (a budget bound), and the
//    AMM computes the matching quote side ITSELF from its own live reserves.
//    Ratio-correctness is true by construction at execution-time reserves --
//    no tolerance window, no ratio race, nothing for an interleaving actor
//    to skew. All rounding residue stays as quote in the caller's account
//    (faction tokens never rest between fires -- the 163 Kani target).
//
// 4. UNIFORM RE-ENTRANCY DISCIPLINE: the same `!pool.locked` constraint +
//    lock/unlock pattern as the swap lanes, verbatim. The transfer-hook CPI
//    during the faction deposit is the real re-entry surface this guard
//    exists for (hook-token AMMs are the one Solana niche where an explicit
//    guard earns its keep).
//
// 5. SLOT-AGNOSTIC: the faction mint may be side A OR side B (canonical
//    byte-ordering of the mint pair decides). Orientation is resolved from
//    the pool's STORED fields, never assumed.

// ---------------------------------------------------------------------------
// Pure helpers (pub: exercised directly by the gate test suite now and the
// Phase-163 Kani harnesses later)
// ---------------------------------------------------------------------------

/// Returns true if the given key is the Token-2022 program.
/// (Local mirror of the swap-lane helper -- it is file-private there.)
fn is_t22(key: &Pubkey) -> bool {
    *key == anchor_spl::token_2022::ID
}

/// Resolve which pool side is the faction mint from the STORED mint pair.
///
/// Returns `Ok(true)` when side A is CRIME or FRAUD, `Ok(false)` when side B
/// is. Exactly one side must be a faction mint; both and neither reject.
pub fn faction_side_is_a(mint_a: &Pubkey, mint_b: &Pubkey) -> std::result::Result<bool, AmmError> {
    let crime = crime_mint();
    let fraud = fraud_mint();
    let a_is_faction = *mint_a == crime || *mint_a == fraud;
    let b_is_faction = *mint_b == crime || *mint_b == fraud;
    match (a_is_faction, b_is_faction) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        _ => Err(AmmError::NotFactionPool),
    }
}

/// Map raw stored reserves to (reserve_faction, reserve_quote) per orientation.
///
/// Kept as a pure function so the flipped-world claim ("orientation reads
/// stored fields") is unit-testable without a chain.
pub fn oriented_reserves(faction_is_a: bool, reserve_a: u64, reserve_b: u64) -> (u64, u64) {
    if faction_is_a {
        (reserve_a, reserve_b)
    } else {
        (reserve_b, reserve_a)
    }
}

/// Compute the matching quote side for a faction-side-exact deposit:
///
/// ```text
/// quote_amount = floor(faction_amount * reserve_quote / reserve_faction)
/// ```
///
/// evaluated in u128 (two u64 factors can never overflow a u128 product).
///
/// PAIRING: the Phase-172 bot mirror must keep this floor division in lockstep
/// so its `max_quote` budget cannot be exceeded by a correct caller.
///
/// Error semantics (all genuine-fault reverts -- the sole caller pre-checks,
/// so hitting any of these means a real bug upstream, and loud is correct):
/// - `ZeroLiquidity`: the quote side floors to zero. Depositing would create a
///   one-sided add (faction in, nothing matching) -- refused.
/// - `ExceedsMaxQuote`: the AMM-computed quote side is above the caller's budget
///   bound. Checked in u128 space BEFORE narrowing, so a quotient beyond
///   u64::MAX is caught here rather than wrapping.
/// - `Overflow`: division by zero (an uninitialized/drained token reserve --
///   unreachable through normal pool life) or a failed narrowing (statically
///   unreachable once the `max_quote` bound held; kept as belt-and-suspenders).
pub fn compute_quote_side(
    faction_amount: u64,
    reserve_quote: u64,
    reserve_faction: u64,
    max_quote: u64,
) -> std::result::Result<u64, AmmError> {
    // u64::MAX^2 = 2^128 - 2^65 + 1 < u128::MAX, so the product cannot
    // overflow; checked_mul is pure belt-and-suspenders.
    let numerator = (faction_amount as u128)
        .checked_mul(reserve_quote as u128)
        .ok_or(AmmError::Overflow)?;
    // checked_div returns None only on a zero divisor.
    let quote_128 = numerator
        .checked_div(reserve_faction as u128)
        .ok_or(AmmError::Overflow)?;
    if quote_128 == 0 {
        return Err(AmmError::ZeroLiquidity);
    }
    if quote_128 > max_quote as u128 {
        return Err(AmmError::ExceedsMaxQuote);
    }
    u64::try_from(quote_128).map_err(|_| AmmError::Overflow)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Add double-sided liquidity to a faction pool: transfer `faction_amount`
/// plus the AMM-computed matching quote amount from an approved ArbConfig
/// wallet into the vaults, and increment both reserves.
///
/// Follows strict CEI (Checks-Effects-Interactions) ordering, mirroring the
/// swap lanes:
/// 1. CHECKS: validate the ArbConfig wallet, inputs, and orientation; compute quote
/// 2. EFFECTS: increment pool reserves
/// 3. INTERACTIONS: execute the two deposit transfers
/// 4. POST-INTERACTION: clear the re-entrancy guard, emit event
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, AddLiquidityDoubleSided<'info>>,
    faction_amount: u64,
    max_quote: u64,
) -> Result<()> {
    // =====================================================================
    // CHECKS
    // =====================================================================

    // 1. Read the canonical ArbConfig and validate the tx-level signer before
    //    any state effect. The scoped borrow is dropped before transfers.
    require!(
        ctx.accounts.arb_config.owner == &epoch_program_id(),
        AmmError::InvalidArbConfig
    );
    let wallet = ctx.accounts.wallet.key();
    let (wallet_a, wallet_b) = {
        let data = ctx.accounts.arb_config.try_borrow_data()?;
        let cfg = ArbConfig::try_deserialize(&mut &data[..])
            .map_err(|_| error!(AmmError::InvalidArbConfig))?;
        (cfg.wallet_a, cfg.wallet_b)
    };
    require!(
        wallet == wallet_a || wallet == wallet_b,
        AmmError::Unauthorized
    );

    // 2. A zero faction side would be a one-sided (or empty) add -- the caller
    //    only calls when it holds a just-bought faction balance, so zero here
    //    is a genuine fault upstream.
    require!(faction_amount > 0, AmmError::ZeroLiquidity);

    // A fee schedule on either side would make the nominal reserve increments
    // differ from the tokens actually received by the vaults.
    validate_amount_preserving_mint(&ctx.accounts.mint_a.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;
    validate_amount_preserving_mint(&ctx.accounts.mint_b.to_account_info())
        .map_err(|_| error!(AmmError::UnsupportedTransferFee))?;

    // 3. Save immutable pool values BEFORE any mutable access (the swap-lane
    //    borrow discipline: capture upfront, mutate after).
    let mint_a_key = ctx.accounts.pool.mint_a;
    let mint_b_key = ctx.accounts.pool.mint_b;
    let reserve_a = ctx.accounts.pool.reserve_a;
    let reserve_b = ctx.accounts.pool.reserve_b;
    let token_program_a_key = ctx.accounts.pool.token_program_a;
    let token_program_b_key = ctx.accounts.pool.token_program_b;

    // 4. Set the re-entrancy guard (the Anchor constraint already verified
    //    the pool arrived unlocked). WHY: the transfer-hook CPI fired by the
    //    faction deposit below hands execution to another program mid-flight;
    //    the guard makes any re-entering pool touch fail loudly instead of
    //    operating on half-updated state. On revert this write rolls back.
    ctx.accounts.pool.locked = true;

    // 5. SLOT-AGNOSTIC orientation from STORED fields: never assume which
    //    canonical side contains the faction mint.
    let faction_is_a = faction_side_is_a(&mint_a_key, &mint_b_key)?;
    let (reserve_faction, reserve_quote) = oriented_reserves(faction_is_a, reserve_a, reserve_b);
    let (faction_mint_key, quote_mint_key, faction_tp_key, quote_tp_key) = if faction_is_a {
        (
            mint_a_key,
            mint_b_key,
            token_program_a_key,
            token_program_b_key,
        )
    } else {
        (
            mint_b_key,
            mint_a_key,
            token_program_b_key,
            token_program_a_key,
        )
    };

    // 6. Defense in depth: the caller's source accounts must sit on the
    //    oriented mints. The token programs would reject a mismatch inside
    //    transfer_checked anyway; failing here is earlier and clearer.
    require!(
        ctx.accounts.caller_faction.mint == faction_mint_key,
        AmmError::InvalidMint
    );
    require!(
        ctx.accounts.caller_quote.mint == quote_mint_key,
        AmmError::InvalidMint
    );

    // 7. The AMM computes the quote side itself at its OWN live reserves --
    //    ratio-correct by construction (see compute_quote_side for the error
    //    semantics; every failure is a genuine-fault revert).
    let quote_amount =
        compute_quote_side(faction_amount, reserve_quote, reserve_faction, max_quote)?;

    // =====================================================================
    // EFFECTS (reserve increments BEFORE the transfer CPIs -- CEI)
    // =====================================================================

    // 8. Checked adds into the ORIENTED sides. WHY before the transfers: if
    //    a hook-mediated re-entry ever slipped past the guard, it would see
    //    reserves already accounting for the incoming deposit -- the
    //    conservative direction (pool claims more than it holds for the
    //    instruction's duration, never less).
    if faction_is_a {
        ctx.accounts.pool.reserve_a = reserve_a
            .checked_add(faction_amount)
            .ok_or(AmmError::Overflow)?;
        ctx.accounts.pool.reserve_b = reserve_b
            .checked_add(quote_amount)
            .ok_or(AmmError::Overflow)?;
    } else {
        ctx.accounts.pool.reserve_a = reserve_a
            .checked_add(quote_amount)
            .ok_or(AmmError::Overflow)?;
        ctx.accounts.pool.reserve_b = reserve_b
            .checked_add(faction_amount)
            .ok_or(AmmError::Overflow)?;
    }

    // Anchor serializes Account<T> wrappers at instruction exit. Flush the
    // CEI effects now so any interaction-time observer sees locked=true and
    // the conservative reserve state. A later CPI error still rolls this
    // write back atomically with the transaction.
    ctx.accounts.pool.exit(ctx.program_id)?;

    // =====================================================================
    // INTERACTIONS (deposit transfers: caller accounts -> vaults)
    // =====================================================================

    // The approved wallet owns both caller accounts and signs both deposits
    // directly, so no PDA signer seeds are required.
    let wallet_info = ctx.accounts.wallet.to_account_info();

    // Oriented account references (materialized after the pool writes; the
    // borrows are field-disjoint).
    let (faction_vault, quote_vault, faction_mint_acc, quote_mint_acc, faction_tp, quote_tp) =
        if faction_is_a {
            (
                &ctx.accounts.vault_a,
                &ctx.accounts.vault_b,
                &ctx.accounts.mint_a,
                &ctx.accounts.mint_b,
                &ctx.accounts.token_program_a,
                &ctx.accounts.token_program_b,
            )
        } else {
            (
                &ctx.accounts.vault_b,
                &ctx.accounts.vault_a,
                &ctx.accounts.mint_b,
                &ctx.accounts.mint_a,
                &ctx.accounts.token_program_b,
                &ctx.accounts.token_program_a,
            )
        };
    let faction_decimals = faction_mint_acc.decimals;
    let quote_decimals = quote_mint_acc.decimals;

    // 9. Faction deposit first. Token-2022 in practice (CRIME/FRAUD carry the
    //    transfer hook), routed by the STORED token program like the swap
    //    lanes. The repo helper forwards remaining_accounts because Anchor's
    //    built-in transfer_checked does NOT (the initialize_pool precedent);
    //    remaining_accounts carry the hook group for the faction mint. Pools
    //    currently have exactly one hooked side, so forwarding the whole list to
    //    the one hooked transfer is correct (VH-I001 contract).
    if is_t22(&faction_tp_key) {
        transfer_t22_checked(
            &faction_tp.to_account_info(),
            &ctx.accounts.caller_faction.to_account_info(),
            &faction_mint_acc.to_account_info(),
            &faction_vault.to_account_info(),
            &wallet_info,
            faction_amount,
            faction_decimals,
            &[],
            ctx.remaining_accounts,
        )?;
    } else {
        transfer_spl(
            &faction_tp.to_account_info(),
            &ctx.accounts.caller_faction.to_account_info(),
            &faction_mint_acc.to_account_info(),
            &faction_vault.to_account_info(),
            &wallet_info,
            faction_amount,
            faction_decimals,
            &[],
        )?;
    }

    // 10. Quote deposit. The stored-program routing supports either classic
    //     SPL Token or Token-2022 quote assets.
    if is_t22(&quote_tp_key) {
        transfer_t22_checked(
            &quote_tp.to_account_info(),
            &ctx.accounts.caller_quote.to_account_info(),
            &quote_mint_acc.to_account_info(),
            &quote_vault.to_account_info(),
            &wallet_info,
            quote_amount,
            quote_decimals,
            &[],
            ctx.remaining_accounts,
        )?;
    } else {
        transfer_spl(
            &quote_tp.to_account_info(),
            &ctx.accounts.caller_quote.to_account_info(),
            &quote_mint_acc.to_account_info(),
            &quote_vault.to_account_info(),
            &wallet_info,
            quote_amount,
            quote_decimals,
            &[],
        )?;
    }

    // =====================================================================
    // POST-INTERACTION
    // =====================================================================

    // 11. Clear the re-entrancy guard.
    ctx.accounts.pool.locked = false;

    // 12. Emit the deposit event (the indexer + FE-03 arb-activity feed
    //     decode this; fields mirror the SwapEvent shape).
    let clock = Clock::get()?;
    emit!(LiquidityAddedEvent {
        pool: ctx.accounts.pool.key(),
        quote_amount,
        faction_amount,
        reserve_a: ctx.accounts.pool.reserve_a,
        reserve_b: ctx.accounts.pool.reserve_b,
        slot: clock.slot,
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

/// Accounts for `add_liquidity_double_sided` (the protocol-only Policy-B
/// deposit).
///
/// The signer is gated through the Epoch Program's canonical ArbConfig PDA,
/// and the caller-side token accounts must be owned by that same wallet.
///
/// Declaration order is load-bearing: Anchor validates fields top-down, so
/// the wallet and ArbConfig gate are the FIRST checks any caller hits.
#[derive(Accounts)]
pub struct AddLiquidityDoubleSided<'info> {
    /// Transaction-level signer; handler requires ArbConfig wallet A or B.
    pub wallet: Signer<'info>,

    /// Epoch Program's canonical ArbConfig PDA.
    /// CHECK: owner + discriminator + field read validated in handler
    /// (GAP-1 idiom, spec §14.2).
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub arb_config: AccountInfo<'info>,

    /// Pool state PDA. Mutable for reserve increments and the re-entrancy
    /// guard. Seeds validate this is the canonical pool for its mint pair;
    /// the `!pool.locked` constraint is the same uniform guard the swap
    /// lanes enforce.
    #[account(
        mut,
        seeds = [POOL_SEED, pool.mint_a.as_ref(), pool.mint_b.as_ref()],
        bump = pool.bump,
        constraint = pool.initialized @ AmmError::PoolNotInitialized,
        constraint = !pool.locked @ AmmError::PoolLocked,
    )]
    pub pool: Account<'info, PoolState>,

    /// Vault A: PDA-owned token account holding reserve A.
    /// Validated against pool state to prevent vault substitution.
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

    /// Mint A: decimals for transfer_checked + orientation cross-check.
    #[account(constraint = mint_a.key() == pool.mint_a @ AmmError::InvalidMint)]
    pub mint_a: InterfaceAccount<'info, Mint>,

    /// Mint B: decimals for transfer_checked + orientation cross-check.
    #[account(constraint = mint_b.key() == pool.mint_b @ AmmError::InvalidMint)]
    pub mint_b: InterfaceAccount<'info, Mint>,

    /// Approved wallet's faction account -- source of the exact faction side.
    #[account(
        mut,
        constraint = caller_faction.owner == wallet.key() @ AmmError::Unauthorized,
    )]
    pub caller_faction: InterfaceAccount<'info, TokenAccount>,

    /// Approved wallet's quote account -- source of the computed quote side.
    #[account(
        mut,
        constraint = caller_quote.owner == wallet.key() @ AmmError::Unauthorized,
    )]
    pub caller_quote: InterfaceAccount<'info, TokenAccount>,

    /// Token program for mint A (classic SPL or Token-2022).
    /// Validated against pool state to prevent program substitution.
    #[account(constraint = token_program_a.key() == pool.token_program_a @ AmmError::InvalidTokenProgram)]
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token program for mint B (classic SPL or Token-2022).
    #[account(constraint = token_program_b.key() == pool.token_program_b @ AmmError::InvalidTokenProgram)]
    pub token_program_b: Interface<'info, TokenInterface>,
}
