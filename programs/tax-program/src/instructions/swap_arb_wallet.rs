//! swap_arb_wallet: tax-free bidirectional AMM swaps for authorized ArbConfig wallets.
//!
//! The lane is generic over every AMM pool containing exactly one canonical CRIME or
//! FRAUD side. All mint, vault, and user-token accounts remain positional A/B; the bot
//! computes `direction` from pool orientation client-side and supplies it as 0 or 1.
//!
//! The transaction signer must match `wallet_a` or `wallet_b` in the Epoch Program's
//! canonical ArbConfig PDA. The lane applies no protocol tax, touches no sweep account,
//! and is deliberately not pause-gated. Only the AMM LP fee applies.
//!
//! ## Min-out guard (defense in depth)
//!
//! A profit-seeking leg enforces `min_out` at TWO layers:
//!   1. BELT: pass `min_out` through to the AMM `swap_sol_pool` as `minimum_amount_out`
//!      so the AMM's own slippage check (`amount_out >= minimum_amount_out`) reverts.
//!   2. SUSPENDERS: snapshot the destination token-account balance BEFORE the CPI,
//!      reload AFTER, and `require!(realized_out >= min_out)` lane-side — so the lane
//!      itself proves the realized output, independent of the AMM's internal accounting.
//!
//! ## CPI depth
//!
//! The wallet calls Tax directly; Tax CPIs to the AMM, which may CPI through Token-2022
//! to the transfer hook. Do not add another CPI layer to this instruction path.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{amm_program_id, epoch_program_id, ARB_CONFIG_SEED, SWAP_AUTHORITY_SEED};
use crate::errors::TaxError;
use crate::helpers::faction_pool_reader;
use crate::state::ArbConfig;

/// First 8 bytes of sha256("global:swap_sol_pool").
const AMM_SWAP_SOL_POOL_DISCRIMINATOR: [u8; 8] = [0xde, 0x80, 0x1e, 0x7b, 0x55, 0x27, 0x91, 0x8a];

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Execute a tax-free, bidirectional swap for an authorized ArbConfig wallet.
///
/// Flow (strict CEI -- the only state-changing INTERACTION is the AMM CPI):
/// 1. CHECKS: validate the ArbConfig wallet, pool identity, input, and direction.
/// 2. Snapshot the destination token-account balance BEFORE the CPI.
/// 3. INTERACTION: build + execute the AMM CPI with `swap_authority` PDA signing,
///    forwarding `min_out` as the AMM's `minimum_amount_out` (belt).
/// 4. Reload the destination account, compute `realized_out`, and enforce the
///    lane-side `realized_out >= min_out` guard (suspenders).
/// 5. Emit `ArbSwapExecuted`.
///
/// NO pause check, tax calculation, sweep interaction, or distribution occurs.
///
/// # Arguments
/// * `amount_in` - Amount of positional token A or B to swap.
/// * `direction` - 0 = AtoB, 1 = BtoA; computed by the bot from pool orientation.
/// * `min_out`   - Minimum realized output sized by the caller. Enforced by
///                 BOTH the AMM CPI and the lane-side re-read.
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SwapArbWallet<'info>>,
    amount_in: u64,
    direction: u8,
    min_out: u64,
) -> Result<()> {
    // =========================================================================
    // 1. CHECKS -- validate ArbConfig wallet, input, direction, and pool identity
    // =========================================================================
    require_keys_eq!(
        *ctx.accounts.arb_config.owner,
        epoch_program_id(),
        TaxError::InvalidArbConfig
    );
    let wallet = ctx.accounts.wallet.key();
    let (wallet_a, wallet_b) = {
        let data = ctx.accounts.arb_config.try_borrow_data()?;
        let config = ArbConfig::try_deserialize(&mut &data[..])
            .map_err(|_| error!(TaxError::InvalidArbConfig))?;
        (config.wallet_a, config.wallet_b)
    };
    require!(
        wallet == wallet_a || wallet == wallet_b,
        TaxError::UnauthorizedArbWallet
    );

    require!(amount_in > 0, TaxError::InsufficientInput);

    // Validate caller-computed positional direction: 0 = AtoB, 1 = BtoA.
    require!(direction <= 1, TaxError::InvalidPoolType);

    // Validate exactly one CRIME/FRAUD identity against both canonical faction
    // mints. The compact boolean is used only by the accounting event.
    let pool_view = faction_pool_reader::read_faction_pool(&ctx.accounts.pool)?;

    // =========================================================================
    // 2. Snapshot the destination balance BEFORE the CPI (suspenders, part 1)
    //
    //    The output account depends on direction:
    //    - AtoB (buy):  output = token B -> destination is user_token_b
    //    - BtoA (sell): output = token A -> destination is user_token_a
    // =========================================================================
    let balance_before: u64 = if direction == 0 {
        ctx.accounts.user_token_b.amount
    } else {
        ctx.accounts.user_token_a.amount
    };

    // =========================================================================
    // 3. Build and execute the AMM CPI (the sole INTERACTION)
    // =========================================================================

    // 3a. Build swap_authority PDA signer seeds.
    let swap_authority_seeds: &[&[u8]] = &[SWAP_AUTHORITY_SEED, &[ctx.bumps.swap_authority]];

    // 3b. Build account metas for AMM swap_sol_pool.
    //     Order matches AMM's SwapSolPool struct (see amm/src/instructions/swap_sol_pool.rs):
    //       swap_authority, pool, vault_a, vault_b, mint_a, mint_b,
    //       user_token_a, user_token_b, user, token_program_a, token_program_b
    //
    //     The wallet owns its working token accounts and signs the user->vault input
    //     transfer directly. Its signer privilege propagates into the CPI, while the
    //     Tax `swap_authority` PDA signs the vault->user output leg below.
    let mut account_metas = vec![
        AccountMeta::new_readonly(ctx.accounts.swap_authority.key(), true), // signer (PDA-signed below)
        AccountMeta::new(ctx.accounts.pool.key(), false),
        AccountMeta::new(ctx.accounts.pool_vault_a.key(), false),
        AccountMeta::new(ctx.accounts.pool_vault_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.mint_b.key(), false),
        AccountMeta::new(ctx.accounts.user_token_a.key(), false),
        AccountMeta::new(ctx.accounts.user_token_b.key(), false),
        AccountMeta::new_readonly(ctx.accounts.wallet.key(), true), // wallet as AMM "user"
        AccountMeta::new_readonly(ctx.accounts.token_program_a.key(), false),
        AccountMeta::new_readonly(ctx.accounts.token_program_b.key(), false),
    ];

    // 3c. Forward remaining_accounts for transfer-hook support.
    //     The AMM passes these to the Token-2022 transfer_checked calls.
    for account in ctx.remaining_accounts.iter() {
        if account.is_writable {
            account_metas.push(AccountMeta::new(account.key(), account.is_signer));
        } else {
            account_metas.push(AccountMeta::new_readonly(account.key(), account.is_signer));
        }
    }

    // 3d. Build instruction data for AMM swap_sol_pool.
    //     Format: discriminator (8) + amount_in (8) + direction (1) + minimum_out (8)
    //
    //     Direction passes through (0=AtoB, 1=BtoA), and `min_out` is forwarded
    //     unchanged as the AMM's `minimum_amount_out`.
    //
    //     minimum_out = `min_out` (BELT): the AMM enforces `amount_out >= minimum_out`
    //     internally and reverts if the pool degraded. This is the profit-seeking
    //     delta vs the Carnage lane's hardcoded 0.

    let mut ix_data = Vec::with_capacity(25);
    ix_data.extend_from_slice(&AMM_SWAP_SOL_POOL_DISCRIMINATOR);
    ix_data.extend_from_slice(&amount_in.to_le_bytes());
    ix_data.push(direction); // SwapDirection: 0=AtoB, 1=BtoA
    ix_data.extend_from_slice(&min_out.to_le_bytes());

    // 3e. Build the instruction.
    let ix = Instruction {
        program_id: ctx.accounts.amm_program.key(),
        accounts: account_metas,
        data: ix_data,
    };

    // 3f. Build account infos for the CPI (same order as account_metas, plus AMM program).
    let mut account_infos = vec![
        ctx.accounts.swap_authority.to_account_info(),
        ctx.accounts.pool.to_account_info(),
        ctx.accounts.pool_vault_a.to_account_info(),
        ctx.accounts.pool_vault_b.to_account_info(),
        ctx.accounts.mint_a.to_account_info(),
        ctx.accounts.mint_b.to_account_info(),
        ctx.accounts.user_token_a.to_account_info(),
        ctx.accounts.user_token_b.to_account_info(),
        ctx.accounts.wallet.to_account_info(),
        ctx.accounts.token_program_a.to_account_info(),
        ctx.accounts.token_program_b.to_account_info(),
    ];

    // Forward remaining_accounts for the transfer hook.
    for account in ctx.remaining_accounts.iter() {
        account_infos.push(account.clone());
    }

    // Add the AMM program account info (required for the CPI).
    account_infos.push(ctx.accounts.amm_program.to_account_info());

    // 3g. Execute the CPI with the swap_authority PDA signature.
    invoke_signed(&ix, &account_infos, &[swap_authority_seeds])?;

    // =========================================================================
    // 4. Lane-side min-out guard (suspenders, part 2)
    //
    //    Reload the destination account to pick up the post-CPI balance, then
    //    compute the realized output as a balance diff and assert it cleared
    //    `min_out`. checked_sub guards the (impossible-but-defensive) underflow.
    // =========================================================================
    let balance_after: u64 = if direction == 0 {
        ctx.accounts.user_token_b.reload()?;
        ctx.accounts.user_token_b.amount
    } else {
        ctx.accounts.user_token_a.reload()?;
        ctx.accounts.user_token_a.amount
    };

    let realized_out = balance_after
        .checked_sub(balance_before)
        .ok_or(TaxError::SlippageExceeded)?;

    require!(realized_out >= min_out, TaxError::SlippageExceeded);

    // =========================================================================
    // 5. Emit the complete tax-free arb accounting event.
    // =========================================================================
    emit!(crate::events::ArbSwapExecuted {
        pool: ctx.accounts.pool.key(),
        quote_mint: pool_view.quote_mint,
        is_crime: pool_view.is_crime,
        direction,
        wallet,
        amount_in,
        realized_out,
        min_out,
        slot: Clock::get()?.slot,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Account struct
// ---------------------------------------------------------------------------

/// Accounts for `swap_arb_wallet` (tax-free, ArbConfig-wallet-gated AMM swaps).
///
/// Pool-facing accounts stay positional A/B. The wallet computes direction from pool
/// orientation client-side; this account surface remains direction-agnostic and has
/// no tax-distribution or sweep accounts.
#[derive(Accounts)]
pub struct SwapArbWallet<'info> {
    /// Transaction signer; handler requires ArbConfig wallet A or B.
    #[account(mut)]
    pub wallet: Signer<'info>,

    /// Epoch Program's canonical ArbConfig PDA.
    /// CHECK: PDA address is constrained here; owner, discriminator, and wallet
    /// fields are validated in the handler via the GAP-1 cross-program idiom.
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub arb_config: AccountInfo<'info>,

    /// Tax Program's `swap_authority` PDA -- signs the AMM CPI.
    /// Same derivation as `swap_sol_buy`/`swap_sol_sell`/`swap_exempt`.
    ///
    /// CHECK: PDA derived from seeds, used as signer for the CPI.
    #[account(
        seeds = [SWAP_AUTHORITY_SEED],
        bump,
    )]
    pub swap_authority: AccountInfo<'info>,

    // === Pool State (AMM) ===
    /// AMM pool state -- mutable for reserve updates.
    /// CHECK: Validated in the AMM CPI.
    #[account(mut)]
    pub pool: AccountInfo<'info>,

    // === Pool Vaults (positional A/B) ===
    /// Pool vault for positional token A.
    #[account(mut)]
    pub pool_vault_a: InterfaceAccount<'info, TokenAccount>,

    /// Pool vault for positional token B.
    #[account(mut)]
    pub pool_vault_b: InterfaceAccount<'info, TokenAccount>,

    // === Mints ===
    /// Positional mint A.
    pub mint_a: InterfaceAccount<'info, Mint>,

    /// Positional mint B.
    pub mint_b: InterfaceAccount<'info, Mint>,

    // === Wallet working token accounts (positional A/B) ===
    /// Wallet's token-A account; source or destination according to direction.
    #[account(mut)]
    pub user_token_a: InterfaceAccount<'info, TokenAccount>,

    /// Wallet's token-B account; source or destination according to direction.
    #[account(mut)]
    pub user_token_b: InterfaceAccount<'info, TokenAccount>,

    // === Programs ===
    /// AMM Program for the swap CPI.
    /// CHECK: Address validated against the known AMM program ID.
    #[account(address = amm_program_id() @ TaxError::InvalidAmmProgram)]
    pub amm_program: AccountInfo<'info>,

    /// Token program for positional mint A (SPL Token or Token-2022).
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token program for positional mint B (SPL Token or Token-2022).
    pub token_program_b: Interface<'info, TokenInterface>,

    /// System program (may be needed for hook accounts).
    pub system_program: Program<'info, System>,
}
