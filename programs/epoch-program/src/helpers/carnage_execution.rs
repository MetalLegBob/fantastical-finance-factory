//! Shared Carnage execution logic.
//!
//! Contains the CarnageAccounts struct, execute_carnage_core() function,
//! and all 7 helper functions used by both execute_carnage (fallback)
//! and execute_carnage_atomic handlers.
//!
//! Extracted in Phase 82 to eliminate ~1800 lines of duplication across
//! two near-identical 1000-line files. Every Carnage bug fix previously
//! had to be applied twice (REG-001 burn mint, sell proceeds, slippage floors).
//!
//! CRITICAL CPI DEPTH: The swap path is exactly at Solana's limit:
//!   execute_carnage[_atomic] (entry) -> Tax::swap_exempt (1)
//!   -> AMM::swap_sol_pool (2) -> Token-2022::transfer_checked (3)
//!   -> Transfer Hook::execute (4) -- SOLANA LIMIT
//!
//! DO NOT add any CPI calls to the swap path.
//!
//! The SOL->WSOL wrap calls (system_program::transfer + sync_native) execute
//! BEFORE the swap at CPI depth 0, so they do NOT impact the swap depth chain.
//!
//! Source: Carnage_Fund_Spec.md Sections 8-10, 13.2-13.3

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use anchor_spl::token_interface::{TokenAccount, TokenInterface};

use crate::constants::{
    AMM_FEE_BPS, CARNAGE_CAP_K_DEN, CARNAGE_CAP_K_NUM, CARNAGE_FUND_SEED, CARNAGE_SIGNER_SEED,
    CARNAGE_SOL_VAULT_SEED, SWAP_EXEMPT_DISCRIMINATOR,
};
use crate::errors::EpochError;
use crate::events::CarnageExecuted;
use crate::state::{CarnageAction, CarnageFundState, EpochState, Token};

/// Number of extra accounts per mint for Token-2022 Transfer Hook CPI.
///
/// Token-2022's transfer_checked_with_hook needs 4 extra accounts per mint:
///   [extra_account_meta_list, whitelist_source, whitelist_destination, hook_program]
/// The 4th account is the Transfer Hook program ID -- Token-2022 needs it in the
/// account list to CPI into the hook. The SDK's
/// createTransferCheckedWithTransferHookInstruction includes it automatically.
pub const HOOK_ACCOUNTS_PER_MINT: usize = 4;

/// Shared accounts for Carnage execution.
///
/// Contains references to the ~14 accounts shared between execute_carnage
/// (fallback) and execute_carnage_atomic handlers.
///
/// Vaults are mutable references because `.reload()` (re-deserialization
/// after CPI) requires `&mut self`. Pools, mints, and programs are immutable
/// `&AccountInfo` since they're CPI passthroughs that the Tax Program validates.
///
/// Mutable state (`epoch_state`, `carnage_state`) are NOT in this struct --
/// they're separate `&mut` params to execute_carnage_core().
pub struct CarnageAccounts<'a, 'info> {
    /// Carnage signer PDA - signs Tax::swap_exempt CPI
    pub carnage_signer: &'a AccountInfo<'info>,
    /// Carnage SOL vault (holds native SOL as lamports)
    pub sol_vault: &'a AccountInfo<'info>,
    /// Carnage's WSOL token account (for swap_exempt user_token_a)
    /// Mutable: needs reload() after sell CPI to read post-sell WSOL balance
    pub carnage_wsol: &'a mut InterfaceAccount<'info, TokenAccount>,
    /// Carnage CRIME vault (Token-2022 account)
    /// Mutable: needs reload() after burn/sell CPI and after buy CPI
    pub crime_vault: &'a mut InterfaceAccount<'info, TokenAccount>,
    /// Carnage FRAUD vault (Token-2022 account)
    /// Mutable: needs reload() after burn/sell CPI and after buy CPI
    pub fraud_vault: &'a mut InterfaceAccount<'info, TokenAccount>,

    // === Pool Accounts (CRIME/SOL + FRAUD/SOL) ===
    /// CRIME/SOL AMM pool
    pub crime_pool: &'a AccountInfo<'info>,
    /// CRIME/SOL pool's SOL vault
    pub crime_pool_vault_a: &'a AccountInfo<'info>,
    /// CRIME/SOL pool's token vault
    pub crime_pool_vault_b: &'a AccountInfo<'info>,
    /// FRAUD/SOL AMM pool
    pub fraud_pool: &'a AccountInfo<'info>,
    /// FRAUD/SOL pool's SOL vault
    pub fraud_pool_vault_a: &'a AccountInfo<'info>,
    /// FRAUD/SOL pool's token vault
    pub fraud_pool_vault_b: &'a AccountInfo<'info>,

    // === Mints ===
    /// WSOL mint (CPI passthrough, shared by both pools)
    pub mint_a: &'a AccountInfo<'info>,
    /// CRIME token mint (mut: Token-2022 burn decrements supply)
    pub crime_mint: &'a AccountInfo<'info>,
    /// FRAUD token mint (mut: Token-2022 burn decrements supply)
    pub fraud_mint: &'a AccountInfo<'info>,

    // === Programs ===
    /// Tax Program (for swap_exempt CPI)
    pub tax_program: &'a AccountInfo<'info>,
    /// AMM Program (passed to Tax for swap)
    pub amm_program: &'a AccountInfo<'info>,
    /// Tax Program's swap_authority PDA (signs AMM CPI within Tax::swap_exempt)
    pub swap_authority: &'a AccountInfo<'info>,
    /// SPL Token program (for WSOL)
    pub token_program_a: &'a Interface<'info, TokenInterface>,
    /// Token-2022 program (for CRIME/FRAUD)
    pub token_program_b: &'a Interface<'info, TokenInterface>,
    /// System program
    pub system_program: &'a AccountInfo<'info>,
}

/// Core Carnage execution: dispose existing holdings -> buy target token -> update state.
///
/// Implements the full dispose->buy->update flow shared by both handlers.
/// Each handler is responsible for its own entry guard before calling this.
///
/// # Parameters
/// - `accounts`: Shared account references (vaults are mutable for reload)
/// - `epoch_state`: Mutable epoch state for clearing carnage_pending
/// - `carnage_state`: Mutable carnage fund state for holdings + statistics
/// - `remaining_accounts`: Transfer Hook accounts (partitioned internally)
/// - `carnage_signer_bump`: Bump for carnage_signer PDA
/// - `sol_vault_bump`: Bump for sol_vault PDA
/// - `slippage_bps`: Slippage floor (FALLBACK=7500, ATOMIC=8500)
/// - `atomic`: true for atomic path, false for fallback (event field only)
///
/// # Flow
/// 1. Read pending action and target from EpochState
/// 2. Partition remaining_accounts for Transfer Hook
/// 3. Handle existing holdings (Burn/Sell/None)
/// 4. Reload target vault after disposal
/// 5. Buy target token (wrap SOL, execute swap, measure tokens received)
/// 6. Enforce slippage floor
/// 7. Update CarnageFundState and EpochState
/// 8. Emit CarnageExecuted event
pub fn execute_carnage_core<'info>(
    accounts: &mut CarnageAccounts<'_, 'info>,
    epoch_state: &mut Account<'info, EpochState>,
    carnage_state: &mut Account<'info, CarnageFundState>,
    remaining_accounts: &'info [AccountInfo<'info>],
    carnage_signer_bump: u8,
    sol_vault_bump: u8,
    slippage_bps: u64,
    atomic: bool,
) -> Result<()> {
    // Read pending action and target from EXISTING EpochState fields
    let action = CarnageAction::from_u8(epoch_state.carnage_action)
        .ok_or(EpochError::InvalidCarnageTargetPool)?;
    let target =
        Token::from_u8(epoch_state.carnage_target).ok_or(EpochError::InvalidCarnageTargetPool)?;
    let current_epoch = epoch_state.current_epoch;

    // NOTE: Mint validation is handled by Anchor struct constraints:
    // crime_mint.key() == crime_vault.mint, fraud_mint.key() == fraud_vault.mint

    let mut tokens_burned: u64 = 0;
    let mut sol_from_sale: u64 = 0;

    // Build signer seeds for carnage_signer PDA
    let carnage_signer_seeds: &[&[u8]] = &[CARNAGE_SIGNER_SEED, &[carnage_signer_bump]];

    // Build signer seeds for carnage_state PDA (authority for token vaults)
    let carnage_state_bump = carnage_state.bump;
    let carnage_state_seeds: &[&[u8]] = &[CARNAGE_FUND_SEED, &[carnage_state_bump]];

    // Read held token info before mutable operations
    let held_amount = carnage_state.held_amount;
    let held_token = carnage_state.held_token;

    // === Partition remaining_accounts for Transfer Hook ===
    let (sell_hook_accounts, buy_hook_accounts) =
        partition_hook_accounts(&action, &target, held_token, remaining_accounts, atomic);

    // === Step 1: Handle existing holdings ===
    match action {
        CarnageAction::Burn => {
            if held_amount > 0 {
                // Select the correct mint AccountInfo based on held_token.
                // held_token: 1=CRIME, 2=FRAUD
                let burn_mint = if held_token == 1 {
                    accounts.crime_mint.to_account_info()
                } else {
                    accounts.fraud_mint.to_account_info()
                };
                tokens_burned = burn_held_tokens(
                    carnage_state,
                    accounts.crime_vault,
                    accounts.fraud_vault,
                    accounts.token_program_b,
                    carnage_state_seeds,
                    burn_mint,
                )?;
            }
        }
        CarnageAction::Sell => {
            if held_amount > 0 {
                // Sell held tokens -> WSOL via swap_exempt (BtoA direction).
                // WSOL from the sale lands in carnage_wsol. It stays there
                // and is available alongside newly-wrapped SOL for the buy step.

                // Approve carnage_signer as delegate on the held vault.
                // The vaults are owned by carnage_state PDA, but swap_exempt
                // uses carnage_signer as the authority. Token-2022 TransferChecked
                // accepts a delegate with sufficient allowance.
                // This CPI executes at depth 0 -- no impact on swap chain.
                approve_delegate(
                    accounts,
                    held_token,
                    held_amount,
                    carnage_state_seeds,
                    carnage_state,
                )?;

                // Snapshot carnage_wsol balance before sell to measure received WSOL.
                let wsol_before = accounts.carnage_wsol.amount;

                execute_sell_swap(
                    accounts,
                    held_amount,
                    held_token,
                    carnage_signer_seeds,
                    sell_hook_accounts,
                )?;

                // Reload carnage_wsol to get post-sell balance (stale after CPI).
                accounts.carnage_wsol.reload()?;
                let wsol_after = accounts.carnage_wsol.amount;
                sol_from_sale = wsol_after
                    .checked_sub(wsol_before)
                    .ok_or(EpochError::Overflow)?;

                msg!(
                    "Sold {} tokens, received {} WSOL",
                    held_amount,
                    sol_from_sale
                );

                // Clear holdings after sell
                carnage_state.held_token = 0;
                carnage_state.held_amount = 0;
            }
        }
        CarnageAction::None => {
            // BuyOnly path - no holdings to handle
        }
    }

    // === Step 1.5: Reload target vault after disposal ===
    // After burn or sell CPI, Anchor's deserialized account data is stale.
    // If the target vault is the same one we just burned from (e.g. burn CRIME
    // then buy CRIME), we'd read the pre-burn balance for target_vault_before,
    // causing an underflow when calculating tokens_bought.
    // Reload to get the fresh on-chain balance.
    match target {
        Token::Crime => accounts.crime_vault.reload()?,
        Token::Fraud => accounts.fraud_vault.reload()?,
    };

    // === Step 2: Buy target token ===
    // Read the target pool's live pre-buy reserves once. The depth-relative cap and
    // the existing slippage floor deliberately share this post-sell, pre-buy read.
    let target_pool_info = match target {
        Token::Crime => accounts.crime_pool,
        Token::Fraud => accounts.fraud_pool,
    };
    let (reserve_sol, reserve_token) =
        read_pool_reserves(target_pool_info, &accounts.mint_a.key())?;

    // consume_randomness stores the new epoch's tax pair before it marks Carnage
    // pending, so these live fields are the post-flip pair for the selected token.
    let (target_buy_tax_bps, target_sell_tax_bps) = match target {
        Token::Crime => (
            epoch_state.crime_buy_tax_bps,
            epoch_state.crime_sell_tax_bps,
        ),
        Token::Fraud => (
            epoch_state.fraud_buy_tax_bps,
            epoch_state.fraud_sell_tax_bps,
        ),
    };
    let cap = carnage_buy_cap_lamports(target_buy_tax_bps, target_sell_tax_bps, reserve_sol)?;

    // Calculate swap amount: min of available SOL balance and the live cap.
    // Must leave rent-exempt minimum in sol_vault so the PDA survives.
    let sol_balance = accounts.sol_vault.lamports();
    let rent = Rent::get()?;
    let rent_exempt_min = rent.minimum_balance(0); // SystemAccount has 0 data bytes
    let available_sol = sol_balance.saturating_sub(rent_exempt_min);
    let swap_amount = std::cmp::min(available_sol, cap);

    // Combine tax SOL with sell proceeds (if any). On the Sell path, WSOL from
    // selling held tokens already sits in carnage_wsol. Adding it to the tax SOL
    // ensures we buy with the full available amount instead of stranding sell proceeds.
    let total_buy_amount = std::cmp::min(
        swap_amount
            .checked_add(sol_from_sale)
            .ok_or(EpochError::Overflow)?,
        cap,
    );
    // Only wrap the portion not already in carnage_wsol from the sell
    let wrap_amount = total_buy_amount.saturating_sub(sol_from_sale);

    let tokens_bought = if total_buy_amount > 0 {
        // Wrap tax SOL from sol_vault -> carnage_wsol. Sell proceeds (if any)
        // are already in carnage_wsol from step 1, so only wrap the new portion.
        if wrap_amount > 0 {
            wrap_sol_to_wsol(accounts, wrap_amount, sol_vault_bump)?;
        }

        // Snapshot target vault balance before swap to calculate actual tokens received.
        // Safe to read .amount here because we reloaded in step 1.5.
        let target_vault_before = match target {
            Token::Crime => accounts.crime_vault.amount,
            Token::Fraud => accounts.fraud_vault.amount,
        };

        // Execute the buy swap with combined SOL (tax + sell proceeds)
        execute_buy_swap(
            accounts,
            total_buy_amount,
            target,
            carnage_signer_seeds,
            buy_hook_accounts,
        )?;

        // Reload target vault to read post-swap balance.
        // After CPI, Anchor's deserialized account data is stale.
        let target_vault_after = match target {
            Token::Crime => {
                accounts.crime_vault.reload()?;
                accounts.crime_vault.amount
            }
            Token::Fraud => {
                accounts.fraud_vault.reload()?;
                accounts.fraud_vault.amount
            }
        };

        // Actual tokens received = post_balance - pre_balance
        let bought = target_vault_after
            .checked_sub(target_vault_before)
            .ok_or(EpochError::Overflow)?;

        // Slippage floor: actual output must be >= slippage_bps% of expected.
        // Atomic (85%): tight enough to catch extreme manipulation but tolerant
        //   of normal same-TX deviations. Primary MEV defense is atomicity + VRF.
        // Fallback (75%): more lenient -- prioritize execution over optimal price
        //   when in recovery mode. The fallback only runs after the lock window
        //   expires, so the atomic path had first chance.
        // Source: Phase 47 CONTEXT.md
        if reserve_sol > 0 && reserve_token > 0 {
            let expected = (reserve_token as u128)
                .checked_mul(total_buy_amount as u128)
                .and_then(|n| {
                    n.checked_div((reserve_sol as u128).checked_add(total_buy_amount as u128)?)
                })
                .ok_or(EpochError::Overflow)?;
            let expected = u64::try_from(expected).map_err(|_| error!(EpochError::Overflow))?;

            let min_output = (expected as u128)
                .checked_mul(slippage_bps as u128)
                .and_then(|n| n.checked_div(10_000))
                .ok_or(EpochError::Overflow)?;
            let min_output = u64::try_from(min_output).map_err(|_| error!(EpochError::Overflow))?;

            require!(bought >= min_output, EpochError::CarnageSlippageExceeded);
        }

        bought
    } else {
        0
    };

    // === Step 3: Update state ===
    // held_token: 0=None, 1=CRIME, 2=FRAUD (matches Token enum + 1)
    carnage_state.held_token = target.to_u8() + 1;
    carnage_state.held_amount = tokens_bought;
    carnage_state.total_sol_spent = carnage_state
        .total_sol_spent
        .checked_add(total_buy_amount)
        .ok_or(EpochError::Overflow)?;
    carnage_state.total_triggers = carnage_state
        .total_triggers
        .checked_add(1)
        .ok_or(EpochError::Overflow)?;
    carnage_state.last_trigger_epoch = current_epoch;

    // Clear pending flags
    epoch_state.carnage_pending = false;
    epoch_state.carnage_action = CarnageAction::None.to_u8();
    epoch_state.carnage_generation = 0;
    epoch_state.carnage_deadline_slot = 0;
    epoch_state.carnage_lock_slot = 0;
    epoch_state.last_carnage_epoch = current_epoch;

    let label = if atomic { "atomic" } else { "fallback" };
    msg!(
        "Carnage executed ({}): action={}, target={}, sol_spent={}, bought={}, burned={}",
        label,
        action.to_u8(),
        target.to_u8(),
        total_buy_amount,
        tokens_bought,
        tokens_burned
    );

    emit!(CarnageExecuted {
        epoch: current_epoch,
        action: action.to_u8(),
        target: target.to_u8(),
        sol_spent: total_buy_amount,
        tokens_bought,
        tokens_burned,
        sol_from_sale,
        atomic,
    });

    Ok(())
}

/// Round-trip friction a sandwicher faces on one token, in basis points.
///
/// `f = buy_tax + sell_tax + 2 * AMM_FEE_BPS`.
///
/// To capture Carnage's price impact a sandwicher must round-trip the target token:
/// buy it ahead of us, sell it behind us. They are NOT in the arb-exempt lane, so they
/// pay the full tax on both legs plus the 1% LP fee twice.
///
/// **Range is `[1400, 2000]` bps — NOT `[1400, 3000]`.** `derive_taxes` always hands a
/// token exactly one LOW rate and one HIGH rate (cheap side = low buy / high sell,
/// expensive side = high buy / low sell), so `buy + sell` is bounded by
/// `[min LOW + min HIGH, max LOW + max HIGH] = [1200, 1800]`. A token is never
/// expensive on BOTH sides, so the 1400+1400 pairing that would give 3000 is
/// structurally impossible. Pinned by `test_carnage_cap.rs`.
///
/// ⚠️ Do NOT confuse this with the 4% cross-token conversion-arb floor
/// (1% + 1% + 2%) — that is a designed feature of the CRIME↔PROFIT↔FRAUD route, which
/// captures no single-token price impact and is therefore useless to a sandwicher.
pub fn carnage_friction_bps(buy_tax_bps: u16, sell_tax_bps: u16) -> u32 {
    buy_tax_bps as u32 + sell_tax_bps as u32 + 2 * AMM_FEE_BPS as u32
}

/// NS-1: the live-adaptive Carnage buy cap, in lamports.
///
/// ```text
///          reserve_sol * f_bps * CARNAGE_CAP_K_NUM
/// cap  =  ------------------------------------------      ( = (f/k) * reserve_sol )
///              10_000 * CARNAGE_CAP_K_DEN
/// ```
///
/// 🔑 **THE MATH IS NOT RESTATED HERE ON PURPOSE.** The single canonical explanation --
/// the constant-product impact law, why `k` is what it is, the exact sandwich break-even,
/// and the accepted `SOS-H067` residual with its tripwire -- lives on
/// [`crate::constants::CARNAGE_CAP_K_NUM`]. **Read it there.**
///
/// This function previously carried its own copy of that reasoning, and the copy was
/// WRONG: it asserted a buy moves price by `~B/R_sol` (the linear law -- a constant-product
/// spot price moves `(1 + D/R)^2 - 1 ~= 2D/R`, twice as much) and that `k = 2` *"sits us at
/// half that point"* (it sat 3.4-4.8% **ABOVE** break-even). Both claims were false, they
/// sat on the security-critical function a reader opens first, and they **drove two
/// independent 163.5 audit passes to file false CRITICALs.** Deleted by `DEC-01a` (mlbob,
/// 2026-08-13). If you are tempted to re-add an explanation here: don't. Link instead.
///
/// The result is doubly live -- **depth-relative** (it tracks `reserve_sol`, so the cap
/// shrinks with the pool rather than becoming an arbitrary wall) and **tax-pair relative**
/// (bigger buys allowed in higher-friction epochs). It is monotone in both, needs no sqrt
/// and no oracle, and cannot be farmed: letting the carnage vault accumulate does not raise
/// it, because it is not a function of the vault at all.
///
/// 🚨 **NO-DOUBLE-TRUNCATION INVARIANT (locked by DEC-01).** Both multiplications run in
/// `u128` and the division happens **ONCE, at the end**. Do not pre-divide an operand --
/// not `reserve_sol / 10_000`, not `f_bps * NUM / DEN`, not `reserve_sol * NUM / DEN`. Each
/// of those truncates an INPUT and silently under-sizes every cap forever;
/// `cap_never_truncates_an_operand` in `test_carnage_cap.rs` fails on all four shapes.
/// (Splitting the FINAL divide into `/10_000` then `/CARNAGE_CAP_K_DEN` is value-identical
/// by the nested-floor identity `floor(floor(n/a)/b) == floor(n/(a*b))` -- harmless, but
/// two divisions cost BPF compute for nothing, so keep the one.)
///
/// Overflow: the `u128` intermediate is `reserve_sol * f_bps * 25`, whose maximum over the
/// FULL input domain (`u64::MAX` reserve, `u16::MAX` on both tax legs) is ~6.05e25 against
/// `u128::MAX` ~= 3.40e38 -- **5.6e12x of headroom**, so the `checked_*` chain below can
/// only ever fail at the final `u64` narrowing, which the friction bound rules out too.
/// `reserve_sol = 0` yields `cap = 0`, which makes Carnage a no-op rather than a revert
/// against an unseeded pool.
///
/// 🚨 **`reserve_sol` MUST come from a pool the program pins, never one the caller
/// chooses freely** — otherwise a caller picks whichever pool has the deepest SOL side
/// and inflates the very limit that binds them. See
/// `.docs/163.1/NS-1-carnage-cap-derivation.md` §5 for the full chain that makes this
/// safe here (the same account feeds the cap AND the swap, and the swap's mint is
/// pinned all the way back to `carnage_state`).
pub fn carnage_buy_cap_lamports(
    buy_tax_bps: u16,
    sell_tax_bps: u16,
    reserve_sol: u64,
) -> Result<u64> {
    let f_bps = carnage_friction_bps(buy_tax_bps, sell_tax_bps);

    // TWO multiplications, then EXACTLY ONE division -- the DEC-01 locked shape.
    let cap = (reserve_sol as u128)
        .checked_mul(f_bps as u128)
        .and_then(|n| n.checked_mul(CARNAGE_CAP_K_NUM as u128))
        .and_then(|n| n.checked_div(10_000u128 * CARNAGE_CAP_K_DEN as u128))
        .ok_or(EpochError::Overflow)?;

    // f*NUM/(10_000*DEN) = f/k < 1 for every reachable f, so cap < reserve_sol <= u64::MAX.
    u64::try_from(cap).map_err(|_| error!(EpochError::Overflow))
}

/// Partition remaining_accounts into sell and buy hook account slices.
///
/// **Atomic path** (`atomic == true`):
/// Layout: `[CRIME_buy(4), FRAUD_buy(4), held_sell(4)?]`
/// - Buy hooks for both mints (8 accounts) because the VRF-derived target is unknown
///   at TX build time (consume_randomness sets it in the same TX).
/// - Sell hooks for the held token (4 accounts) appended if the fund has holdings.
///   The held token IS known at TX build time (stable across the bundled TX).
///   Sell-direction hooks differ from buy-direction (whitelist PDA positions swapped).
/// This function selects the correct slices using the real target/action.
///
/// **Fallback path** (`atomic == false`):
/// Client reads fresh state and sends pre-selected hooks:
/// Sell: `[sell_hook(4), buy_hook(4)]`, Burn/BuyOnly: `[buy_hook(4)]`.
pub fn partition_hook_accounts<'info>(
    action: &CarnageAction,
    target: &Token,
    _held_token: u8,
    remaining_accounts: &'info [AccountInfo<'info>],
    atomic: bool,
) -> (&'info [AccountInfo<'info>], &'info [AccountInfo<'info>]) {
    if atomic && remaining_accounts.len() >= HOOK_ACCOUNTS_PER_MINT * 2 {
        // Atomic layout: [CRIME_buy(4), FRAUD_buy(4), held_sell(4)?]
        let crime_buy = &remaining_accounts[..HOOK_ACCOUNTS_PER_MINT];
        let fraud_buy = &remaining_accounts[HOOK_ACCOUNTS_PER_MINT..HOOK_ACCOUNTS_PER_MINT * 2];

        let buy_hooks = match target {
            Token::Crime => crime_buy,
            Token::Fraud => fraud_buy,
        };

        if matches!(action, CarnageAction::Sell)
            && remaining_accounts.len() >= HOOK_ACCOUNTS_PER_MINT * 3
        {
            // Sell hooks at [8..12], resolved for sell direction by the client
            let sell_hooks =
                &remaining_accounts[HOOK_ACCOUNTS_PER_MINT * 2..HOOK_ACCOUNTS_PER_MINT * 3];
            (sell_hooks, buy_hooks)
        } else {
            (&remaining_accounts[..0], buy_hooks)
        }
    } else if matches!(action, CarnageAction::Sell)
        && remaining_accounts.len() >= HOOK_ACCOUNTS_PER_MINT * 2
    {
        // Fallback: client sends [sell_hook(4), buy_hook(4)]
        (
            &remaining_accounts[..HOOK_ACCOUNTS_PER_MINT],
            &remaining_accounts[HOOK_ACCOUNTS_PER_MINT..],
        )
    } else {
        // Fallback Burn/BuyOnly: all remaining_accounts are for the buy hook
        (&remaining_accounts[..0], remaining_accounts)
    }
}

/// Burn held tokens from the appropriate vault.
///
/// Token-2022 burns do NOT trigger transfer hooks, so this doesn't
/// consume CPI depth for the hook call.
///
/// NOTE: This function uses raw invoke_signed for Token-2022 burn.
/// The burn instruction is built manually to support Token-2022.
pub fn burn_held_tokens<'info>(
    carnage_state: &mut Account<'info, CarnageFundState>,
    crime_vault: &InterfaceAccount<'info, TokenAccount>,
    fraud_vault: &InterfaceAccount<'info, TokenAccount>,
    token_program: &Interface<'info, TokenInterface>,
    signer_seeds: &[&[u8]],
    burn_mint: AccountInfo<'info>,
) -> Result<u64> {
    let amount = carnage_state.held_amount;
    if amount == 0 {
        return Ok(0);
    }

    // Determine which vault to burn from based on held_token
    // held_token: 0=None, 1=CRIME, 2=FRAUD
    let is_crime = match carnage_state.held_token {
        1 => true,
        2 => false,
        _ => return Ok(0), // No holdings
    };

    let vault = if is_crime {
        crime_vault.to_account_info()
    } else {
        fraud_vault.to_account_info()
    };
    let mint = if is_crime {
        crime_vault.mint
    } else {
        fraud_vault.mint
    };

    // Build burn instruction data manually for Token-2022
    // Instruction discriminator: 8 (Burn)
    // Amount: u64 LE bytes
    let mut burn_data = vec![8u8]; // Burn instruction discriminator
    burn_data.extend_from_slice(&amount.to_le_bytes());

    let burn_ix = Instruction {
        program_id: token_program.key(),
        accounts: vec![
            AccountMeta::new(vault.key(), false), // account to burn from
            AccountMeta::new(mint, false),        // mint
            AccountMeta::new_readonly(carnage_state.key(), true), // authority (carnage_state PDA)
        ],
        data: burn_data,
    };

    // Get carnage_state account info for signing
    let carnage_state_info = carnage_state.to_account_info();

    invoke_signed(
        &burn_ix,
        &[
            vault,
            burn_mint, // FIX REG-001: actual mint AccountInfo (was token_program)
            carnage_state_info,
        ],
        &[signer_seeds],
    )?;

    // Update statistics
    if is_crime {
        carnage_state.total_crime_burned = carnage_state
            .total_crime_burned
            .checked_add(amount)
            .ok_or(EpochError::Overflow)?;
    } else {
        carnage_state.total_fraud_burned = carnage_state
            .total_fraud_burned
            .checked_add(amount)
            .ok_or(EpochError::Overflow)?;
    }

    // Clear holdings
    carnage_state.held_token = 0;
    carnage_state.held_amount = 0;

    msg!(
        "Burned {} tokens from {} vault",
        amount,
        if is_crime { "CRIME" } else { "FRAUD" }
    );

    Ok(amount)
}

/// Wrap SOL from sol_vault PDA into WSOL in carnage_wsol token account.
///
/// Two CPI calls at depth 0 (no impact on swap CPI depth chain):
/// 1. system_program::transfer: sol_vault -> carnage_wsol (signed by sol_vault PDA)
/// 2. spl_token::sync_native: sync carnage_wsol balance (permissionless)
pub fn wrap_sol_to_wsol<'info>(
    accounts: &CarnageAccounts<'_, 'info>,
    amount: u64,
    sol_vault_bump: u8,
) -> Result<()> {
    let sol_vault_seeds: &[&[u8]] = &[CARNAGE_SOL_VAULT_SEED, &[sol_vault_bump]];

    let transfer_ix = anchor_lang::solana_program::system_instruction::transfer(
        &accounts.sol_vault.key(),
        &accounts.carnage_wsol.key(),
        amount,
    );

    invoke_signed(
        &transfer_ix,
        &[
            accounts.sol_vault.to_account_info(),
            accounts.carnage_wsol.to_account_info(),
            accounts.system_program.to_account_info(),
        ],
        &[sol_vault_seeds],
    )?;

    // SyncNative: update WSOL token balance to match lamport balance.
    let sync_native_ix = Instruction {
        program_id: accounts.token_program_a.key(),
        accounts: vec![AccountMeta::new(accounts.carnage_wsol.key(), false)],
        data: vec![17u8], // SyncNative instruction discriminator
    };

    invoke_signed(
        &sync_native_ix,
        &[
            accounts.carnage_wsol.to_account_info(),
            accounts.token_program_a.to_account_info(),
        ],
        &[], // SyncNative is permissionless
    )?;

    msg!("Wrapped {} lamports SOL -> WSOL in carnage_wsol", amount);
    Ok(())
}

/// Execute sell swap: held token -> WSOL via Tax::swap_exempt.
///
/// The WSOL from the sale lands in carnage_wsol. It is NOT unwrapped back to
/// sol_vault -- instead, it's combined with newly-wrapped tax SOL for the
/// subsequent buy step, maximizing the buy amount.
///
/// WSOL balance measurement (before/after delta) is done by the caller
/// because reload() requires mutable access to the account.
pub fn execute_sell_swap<'info>(
    accounts: &CarnageAccounts<'_, 'info>,
    amount: u64,
    held_token: u8,
    carnage_signer_seeds: &[&[u8]],
    hook_accounts: &'info [AccountInfo<'info>],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }

    // Determine source vault based on held token
    // held_token: 0=None, 1=CRIME, 2=FRAUD
    let is_crime = match held_token {
        1 => true,
        2 => false,
        _ => return Ok(()),
    };

    // Select the HELD token's pool (sell uses the pool for what we're selling)
    let (pool, pool_va, pool_vb, ip_mint) = if is_crime {
        (
            accounts.crime_pool,
            accounts.crime_pool_vault_a,
            accounts.crime_pool_vault_b,
            accounts.crime_mint,
        )
    } else {
        (
            accounts.fraud_pool,
            accounts.fraud_pool_vault_a,
            accounts.fraud_pool_vault_b,
            accounts.fraud_mint,
        )
    };

    // Build swap_exempt CPI for sell (direction = 1 = BtoA)
    execute_swap_exempt_cpi(
        accounts,
        amount,
        1, // BtoA (sell token for SOL)
        is_crime,
        carnage_signer_seeds,
        pool,
        pool_va,
        pool_vb,
        ip_mint,
        hook_accounts,
    )?;

    Ok(())
}

/// Execute buy swap: SOL -> target token via Tax::swap_exempt.
///
/// Caller must wrap SOL to WSOL in carnage_wsol BEFORE calling this.
/// Token receipt is measured by the caller via vault balance delta.
pub fn execute_buy_swap<'info>(
    accounts: &CarnageAccounts<'_, 'info>,
    amount: u64,
    target: Token,
    carnage_signer_seeds: &[&[u8]],
    hook_accounts: &'info [AccountInfo<'info>],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }

    let is_crime = matches!(target, Token::Crime);

    // Select the TARGET token's pool (buy uses the pool for what we're buying)
    let (pool, pool_va, pool_vb, ip_mint) = if is_crime {
        (
            accounts.crime_pool,
            accounts.crime_pool_vault_a,
            accounts.crime_pool_vault_b,
            accounts.crime_mint,
        )
    } else {
        (
            accounts.fraud_pool,
            accounts.fraud_pool_vault_a,
            accounts.fraud_pool_vault_b,
            accounts.fraud_mint,
        )
    };

    // Build swap_exempt CPI for buy (direction = 0 = AtoB)
    execute_swap_exempt_cpi(
        accounts,
        amount,
        0, // AtoB (buy token with SOL)
        is_crime,
        carnage_signer_seeds,
        pool,
        pool_va,
        pool_vb,
        ip_mint,
        hook_accounts,
    )?;

    Ok(())
}

/// Execute Tax::swap_exempt CPI.
///
/// This function builds and executes the CPI to Tax Program's swap_exempt.
/// Pool accounts and IP mint are passed as parameters so callers can select
/// the correct pool (held token's pool for sell, target token's pool for buy).
///
/// `hook_accounts` is the SPECIFIC slice of remaining_accounts for this swap's
/// Transfer Hook. For Sell+Buy, the sell and buy swaps get different slices
/// because they transfer different token mints (each with its own hook PDAs).
pub fn execute_swap_exempt_cpi<'info>(
    accounts: &CarnageAccounts<'_, 'info>,
    amount: u64,
    direction: u8,
    is_crime: bool,
    carnage_signer_seeds: &[&[u8]],
    pool: &AccountInfo<'info>,
    pool_vault_a: &AccountInfo<'info>,
    pool_vault_b: &AccountInfo<'info>,
    ip_mint: &AccountInfo<'info>,
    hook_accounts: &'info [AccountInfo<'info>],
) -> Result<()> {
    // Determine user_token_b based on target
    let user_token_b = if is_crime {
        accounts.crime_vault.to_account_info()
    } else {
        accounts.fraud_vault.to_account_info()
    };

    // Build account metas matching SwapExempt struct in Tax Program
    // Order: carnage_authority, swap_authority, pool, pool_vault_a,
    //        pool_vault_b, mint_a, mint_b, user_token_a, user_token_b,
    //        amm_program, token_program_a, token_program_b, system_program
    let mut account_metas = vec![
        AccountMeta::new_readonly(accounts.carnage_signer.key(), true), // carnage_authority (signer)
        AccountMeta::new_readonly(accounts.swap_authority.key(), false), // swap_authority (Tax PDA)
        AccountMeta::new(pool.key(), false),                            // pool
        AccountMeta::new(pool_vault_a.key(), false),                    // pool_vault_a
        AccountMeta::new(pool_vault_b.key(), false),                    // pool_vault_b
        AccountMeta::new_readonly(accounts.mint_a.key(), false),        // mint_a (WSOL)
        AccountMeta::new_readonly(ip_mint.key(), false),                // mint_b (IP token)
        AccountMeta::new(accounts.carnage_wsol.key(), false),           // user_token_a
        AccountMeta::new(user_token_b.key(), false),                    // user_token_b
        AccountMeta::new_readonly(accounts.amm_program.key(), false),   // amm_program
        AccountMeta::new_readonly(accounts.token_program_a.key(), false), // token_program_a
        AccountMeta::new_readonly(accounts.token_program_b.key(), false), // token_program_b
        AccountMeta::new_readonly(accounts.system_program.key(), false), // system_program
    ];

    // Add hook accounts for this specific swap's Transfer Hook
    for account in hook_accounts.iter() {
        if account.is_writable {
            account_metas.push(AccountMeta::new(account.key(), account.is_signer));
        } else {
            account_metas.push(AccountMeta::new_readonly(account.key(), account.is_signer));
        }
    }

    // Build instruction data: discriminator + amount_in + direction + is_crime
    let mut ix_data = Vec::with_capacity(18);
    ix_data.extend_from_slice(&SWAP_EXEMPT_DISCRIMINATOR);
    ix_data.extend_from_slice(&amount.to_le_bytes());
    ix_data.push(direction);
    ix_data.push(u8::from(is_crime));

    let ix = Instruction {
        program_id: accounts.tax_program.key(),
        accounts: account_metas,
        data: ix_data,
    };

    // Build account infos
    let mut account_infos = vec![
        accounts.carnage_signer.to_account_info(),
        accounts.swap_authority.to_account_info(),
        pool.to_account_info(),
        pool_vault_a.to_account_info(),
        pool_vault_b.to_account_info(),
        accounts.mint_a.to_account_info(),
        ip_mint.to_account_info(),
        accounts.carnage_wsol.to_account_info(),
        user_token_b,
        accounts.amm_program.to_account_info(),
        accounts.token_program_a.to_account_info(),
        accounts.token_program_b.to_account_info(),
        accounts.system_program.to_account_info(),
    ];

    // Forward hook accounts for transfer hook
    for account in hook_accounts.iter() {
        account_infos.push(account.clone());
    }

    // Add Tax program account info (required for CPI)
    account_infos.push(accounts.tax_program.to_account_info());

    invoke_signed(&ix, &account_infos, &[carnage_signer_seeds])?;

    Ok(())
}

/// Read reserve_a and reserve_b from a PoolState AccountInfo.
///
/// Returns (reserve_sol, reserve_token) by determining which reserve
/// corresponds to SOL based on the pool's canonical mint ordering.
///
/// PoolState byte layout (Anchor + Borsh):
///   [0..8]    Anchor discriminator
///   [8]       pool_type (1 byte)
///   [9..41]   mint_a (Pubkey, 32 bytes)
///   [41..73]  mint_b (Pubkey, 32 bytes)
///   [73..105] vault_a (Pubkey, 32 bytes)
///   [105..137] vault_b (Pubkey, 32 bytes)
///   [137..145] reserve_a (u64, 8 bytes)
///   [145..153] reserve_b (u64, 8 bytes)
pub fn read_pool_reserves(pool_info: &AccountInfo, wsol_mint_key: &Pubkey) -> Result<(u64, u64)> {
    let data = pool_info.data.borrow();
    require!(data.len() >= 153, EpochError::InvalidCarnageTargetPool);

    let pool_mint_a =
        Pubkey::try_from(&data[9..41]).map_err(|_| error!(EpochError::InvalidCarnageTargetPool))?;

    let reserve_a = u64::from_le_bytes(
        data[137..145]
            .try_into()
            .map_err(|_| error!(EpochError::InvalidCarnageTargetPool))?,
    );
    let reserve_b = u64::from_le_bytes(
        data[145..153]
            .try_into()
            .map_err(|_| error!(EpochError::InvalidCarnageTargetPool))?,
    );

    // If pool's mint_a is WSOL, then reserve_a = SOL, reserve_b = token.
    // Otherwise, reserve_b = SOL, reserve_a = token.
    if pool_mint_a == *wsol_mint_key {
        Ok((reserve_a, reserve_b))
    } else {
        Ok((reserve_b, reserve_a))
    }
}

/// Approve carnage_signer as Token-2022 delegate on the held vault.
///
/// The vaults are owned by carnage_state PDA, but swap_exempt uses
/// carnage_signer as the user authority. Token-2022 TransferChecked
/// accepts a delegate with sufficient allowance.
///
/// This CPI executes at depth 0 -- no impact on the swap CPI chain.
pub fn approve_delegate<'info>(
    accounts: &CarnageAccounts<'_, 'info>,
    held_token: u8,
    amount: u64,
    carnage_state_seeds: &[&[u8]],
    carnage_state: &Account<'info, CarnageFundState>,
) -> Result<()> {
    // held_token: 1=CRIME, 2=FRAUD
    let vault = if held_token == 1 {
        accounts.crime_vault.to_account_info()
    } else {
        accounts.fraud_vault.to_account_info()
    };

    // Token-2022 Approve instruction discriminator = 4
    // Data: [4, amount_le_bytes(8)]
    let mut approve_data = vec![4u8];
    approve_data.extend_from_slice(&amount.to_le_bytes());

    let approve_ix = Instruction {
        program_id: accounts.token_program_b.key(),
        accounts: vec![
            AccountMeta::new(vault.key(), false), // source account
            AccountMeta::new_readonly(accounts.carnage_signer.key(), false), // delegate
            AccountMeta::new_readonly(carnage_state.key(), true), // owner (carnage_state PDA)
        ],
        data: approve_data,
    };

    invoke_signed(
        &approve_ix,
        &[
            vault,
            accounts.carnage_signer.to_account_info(),
            carnage_state.to_account_info(),
        ],
        &[carnage_state_seeds],
    )?;

    msg!(
        "Approved carnage_signer as delegate for {} tokens on {} vault",
        amount,
        if held_token == 1 { "CRIME" } else { "FRAUD" }
    );

    Ok(())
}
