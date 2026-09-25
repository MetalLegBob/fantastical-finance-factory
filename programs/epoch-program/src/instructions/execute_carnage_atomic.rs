//! execute_carnage_atomic instruction.
//!
//! Public instruction for Carnage execution bundled with consume_randomness.
//! Safe to include in every reveal+consume TX for atomic Carnage bundling --
//! no-ops gracefully when carnage_pending is false.
//! Executes burn/sell + buy operations based on EpochState pending fields.
//!
//! CRITICAL CPI DEPTH: The swap path is exactly at Solana's limit:
//!   execute_carnage_atomic (entry) -> Tax::swap_exempt (1)
//!   -> AMM::swap_sol_pool (2) -> Token-2022::transfer_checked (3)
//!   -> Transfer Hook::execute (4) -- SOLANA LIMIT
//!
//! DO NOT add any CPI calls to the swap path.
//!
//! The SOL->WSOL wrap calls (system_program::transfer + sync_native) execute
//! BEFORE the swap at CPI depth 0, so they do NOT impact the swap depth chain.
//!
//! Source: Carnage_Fund_Spec.md Sections 8-10, 13.2

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{TokenAccount, TokenInterface};

use crate::constants::{
    amm_program_id, tax_program_id, CARNAGE_FUND_SEED, CARNAGE_SIGNER_SEED,
    CARNAGE_SLIPPAGE_BPS_ATOMIC, CARNAGE_SOL_VAULT_SEED, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED,
};
use crate::errors::EpochError;
use crate::helpers::carnage_execution::{execute_carnage_core, CarnageAccounts};
use crate::state::{CarnageFundState, EpochState};

/// Accounts for execute_carnage_atomic instruction.
///
/// This instruction is PUBLIC - can be called by anyone. No-ops when carnage_pending
/// is false, enabling safe bundling with consume_randomness in the same transaction.
/// When Carnage triggers, execution proceeds normally. When it doesn't, this is a
/// harmless no-op (~100 CU for account validation + early return).
///
/// STACK BUDGET: State accounts are Box'd to move deserialized data to heap (~247 bytes saved).
/// Pool vaults and mints are AccountInfo because Epoch only forwards them to Tax::swap_exempt CPI --
/// it never reads their data. Tax Program validates them. This avoids ~494 bytes of unnecessary
/// deserialization and keeps the instruction well within the 4096-byte BPF stack frame limit.
#[derive(Accounts)]
pub struct ExecuteCarnageAtomic<'info> {
    /// Caller (anyone - permissionless execution)
    pub caller: Signer<'info>,

    /// Global epoch state (has pending Carnage flags)
    /// NOTE: EpochState already has carnage_pending, carnage_action, carnage_target,
    /// carnage_deadline_slot fields from Phase 23 - we READ these existing fields.
    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
        constraint = epoch_state.safety_version == EPOCH_SAFETY_VERSION
            @ EpochError::SafetyActivationRequired,
    )]
    pub epoch_state: Box<Account<'info, EpochState>>,

    /// Carnage Fund state
    #[account(
        mut,
        seeds = [CARNAGE_FUND_SEED],
        bump = carnage_state.bump,
        constraint = carnage_state.initialized @ EpochError::CarnageNotInitialized,
    )]
    pub carnage_state: Box<Account<'info, CarnageFundState>>,

    /// Carnage signer PDA - signs Tax::swap_exempt CPI
    /// CHECK: PDA derived from known seeds, used as signer for swap_exempt
    #[account(
        seeds = [CARNAGE_SIGNER_SEED],
        bump,
    )]
    pub carnage_signer: AccountInfo<'info>,

    /// Carnage SOL vault (holds native SOL as lamports)
    /// CHECK: PDA derived from known seeds, holds native SOL
    #[account(
        mut,
        seeds = [CARNAGE_SOL_VAULT_SEED],
        bump,
    )]
    pub sol_vault: SystemAccount<'info>,

    /// Carnage's WSOL token account (for swap_exempt user_token_a)
    /// This receives wrapped SOL before swap and unwraps after
    /// Box'd for stack savings (~165 bytes -> 8 bytes)
    #[account(
        mut,
        constraint = carnage_wsol.owner == carnage_signer.key()
            @ EpochError::InvalidCarnageWsolOwner,
    )]
    pub carnage_wsol: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Carnage CRIME vault (Token-2022 account)
    /// Box'd for stack savings (~165 bytes -> 8 bytes)
    #[account(
        mut,
        constraint = crime_vault.key() == carnage_state.crime_vault @ EpochError::InvalidCarnageTargetPool,
    )]
    pub crime_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Carnage FRAUD vault (Token-2022 account)
    /// Box'd for stack savings (~165 bytes -> 8 bytes)
    #[account(
        mut,
        constraint = fraud_vault.key() == carnage_state.fraud_vault @ EpochError::InvalidCarnageTargetPool,
    )]
    pub fraud_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    // === Pool Accounts (CRIME/SOL + FRAUD/SOL) ===
    // Both pools are always provided so the handler can use the held token's pool
    // for sell operations and the target token's pool for buy operations.
    // These are CPI passthroughs -- Tax::swap_exempt validates their contents.
    /// CRIME/SOL AMM pool
    /// CHECK: Owner verified as AMM program, contents validated by Tax during CPI
    #[account(mut, owner = amm_program_id() @ EpochError::InvalidAmmProgram)]
    pub crime_pool: AccountInfo<'info>,

    /// CRIME/SOL pool's SOL vault
    /// CHECK: Validated by Tax Program during swap_exempt CPI
    #[account(mut)]
    pub crime_pool_vault_a: AccountInfo<'info>,

    /// CRIME/SOL pool's token vault
    /// CHECK: Validated by Tax Program during swap_exempt CPI
    #[account(mut)]
    pub crime_pool_vault_b: AccountInfo<'info>,

    /// FRAUD/SOL AMM pool
    /// CHECK: Owner verified as AMM program, contents validated by Tax during CPI
    #[account(mut, owner = amm_program_id() @ EpochError::InvalidAmmProgram)]
    pub fraud_pool: AccountInfo<'info>,

    /// FRAUD/SOL pool's SOL vault
    /// CHECK: Validated by Tax Program during swap_exempt CPI
    #[account(mut)]
    pub fraud_pool_vault_a: AccountInfo<'info>,

    /// FRAUD/SOL pool's token vault
    /// CHECK: Validated by Tax Program during swap_exempt CPI
    #[account(mut)]
    pub fraud_pool_vault_b: AccountInfo<'info>,

    // === Mints ===
    /// WSOL mint (CPI passthrough, shared by both pools)
    /// CHECK: Validated by Tax and AMM programs during swap
    pub mint_a: AccountInfo<'info>,

    /// CRIME token mint (mut: Token-2022 burn decrements supply)
    /// CHECK: Must match crime_vault.mint
    #[account(mut, constraint = crime_mint.key() == crime_vault.mint @ EpochError::InvalidMint)]
    pub crime_mint: AccountInfo<'info>,

    /// FRAUD token mint (mut: Token-2022 burn decrements supply)
    /// CHECK: Must match fraud_vault.mint
    #[account(mut, constraint = fraud_mint.key() == fraud_vault.mint @ EpochError::InvalidMint)]
    pub fraud_mint: AccountInfo<'info>,

    // === Programs ===
    /// Tax Program (for swap_exempt CPI)
    /// CHECK: Address validated against known Tax program ID
    #[account(address = tax_program_id() @ EpochError::InvalidTaxProgram)]
    pub tax_program: AccountInfo<'info>,

    /// AMM Program (passed to Tax for swap)
    /// CHECK: Address validated against known AMM program ID
    #[account(address = amm_program_id() @ EpochError::InvalidAmmProgram)]
    pub amm_program: AccountInfo<'info>,

    /// Tax Program's swap_authority PDA (signs AMM CPI within Tax::swap_exempt)
    /// CHECK: PDA derived from Tax Program seeds, validated during Tax CPI
    pub swap_authority: AccountInfo<'info>,

    /// SPL Token program (for WSOL)
    pub token_program_a: Interface<'info, TokenInterface>,

    /// Token-2022 program (for CRIME/FRAUD)
    pub token_program_b: Interface<'info, TokenInterface>,

    /// System program
    pub system_program: Program<'info, System>,
}

/// Handler for execute_carnage_atomic instruction.
///
/// Can be called by anyone; no-ops when carnage_pending is false.
/// Safe to include in every reveal+consume TX for atomic Carnage bundling.
/// Uses 85% slippage floor (tighter than fallback's 75%).
///
/// Source: Carnage_Fund_Spec.md Sections 8-10, 13.2
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteCarnageAtomic<'info>>,
    generation: u32,
) -> Result<()> {
    let clock = Clock::get()?;
    require!(
        generation == ctx.accounts.epoch_state.current_epoch,
        EpochError::CarnageGenerationMismatch
    );

    // No-op guard: If Carnage wasn't triggered this epoch, return immediately.
    // This allows executeCarnageAtomic to be safely bundled in every
    // reveal+consume TX without knowing the VRF result beforehand.
    // When Carnage doesn't trigger, this is a harmless no-op.
    // When Carnage triggers, execution proceeds normally.
    if !ctx.accounts.epoch_state.carnage_pending {
        return Ok(());
    }

    require!(
        generation == ctx.accounts.epoch_state.carnage_generation,
        EpochError::CarnageGenerationMismatch
    );
    require!(
        clock.slot <= ctx.accounts.epoch_state.carnage_deadline_slot,
        EpochError::CarnageDeadlineExpired
    );

    // Build CarnageAccounts from Context fields
    let mut carnage_accounts = CarnageAccounts {
        carnage_signer: &ctx.accounts.carnage_signer,
        sol_vault: &ctx.accounts.sol_vault.to_account_info(),
        carnage_wsol: &mut ctx.accounts.carnage_wsol,
        crime_vault: &mut ctx.accounts.crime_vault,
        fraud_vault: &mut ctx.accounts.fraud_vault,
        crime_pool: &ctx.accounts.crime_pool,
        crime_pool_vault_a: &ctx.accounts.crime_pool_vault_a,
        crime_pool_vault_b: &ctx.accounts.crime_pool_vault_b,
        fraud_pool: &ctx.accounts.fraud_pool,
        fraud_pool_vault_a: &ctx.accounts.fraud_pool_vault_a,
        fraud_pool_vault_b: &ctx.accounts.fraud_pool_vault_b,
        mint_a: &ctx.accounts.mint_a,
        crime_mint: &ctx.accounts.crime_mint,
        fraud_mint: &ctx.accounts.fraud_mint,
        tax_program: &ctx.accounts.tax_program,
        amm_program: &ctx.accounts.amm_program,
        swap_authority: &ctx.accounts.swap_authority,
        token_program_a: &ctx.accounts.token_program_a,
        token_program_b: &ctx.accounts.token_program_b,
        system_program: &ctx.accounts.system_program.to_account_info(),
    };

    execute_carnage_core(
        &mut carnage_accounts,
        &mut ctx.accounts.epoch_state,
        &mut ctx.accounts.carnage_state,
        ctx.remaining_accounts,
        ctx.bumps.carnage_signer,
        ctx.bumps.sol_vault,
        CARNAGE_SLIPPAGE_BPS_ATOMIC,
        true, // atomic execution
    )
}

#[cfg(test)]
mod tests {
    use crate::constants::CARNAGE_SLIPPAGE_BPS_ATOMIC;

    /// Verify the 85% slippage floor math: expected=1000 tokens.
    /// min_output = 1000 * 8500 / 10000 = 850.
    /// 849 should fail the check, 850 should pass.
    #[test]
    fn test_slippage_floor_rejects_low_output() {
        let expected: u64 = 1000;
        let min_output = (expected as u128)
            .checked_mul(CARNAGE_SLIPPAGE_BPS_ATOMIC as u128)
            .and_then(|n| n.checked_div(10_000))
            .unwrap() as u64;
        assert_eq!(min_output, 850);

        // 849 should fail, 850 should pass
        assert!(849 < min_output);
        assert!(850 >= min_output);
    }

    /// Verify slippage floor handles large values without overflow.
    /// expected = 1 trillion tokens (large Carnage swap).
    /// Uses u128 intermediate arithmetic to avoid u64 overflow.
    #[test]
    fn test_slippage_floor_handles_large_values() {
        let expected: u64 = 1_000_000_000_000;
        let min_output = (expected as u128)
            .checked_mul(CARNAGE_SLIPPAGE_BPS_ATOMIC as u128)
            .and_then(|n| n.checked_div(10_000))
            .unwrap() as u64;
        assert_eq!(min_output, 850_000_000_000);
    }

    /// Zero expected output should result in zero minimum.
    /// This edge case occurs when the pool is empty or swap amount is zero.
    #[test]
    fn test_slippage_floor_zero_expected() {
        let expected: u64 = 0;
        let min_output = (expected as u128)
            .checked_mul(CARNAGE_SLIPPAGE_BPS_ATOMIC as u128)
            .and_then(|n| n.checked_div(10_000))
            .unwrap() as u64;
        assert_eq!(min_output, 0);
    }

    /// Verify the new 85% floor is tighter than the old 50% floor.
    /// Phase 47 upgraded from 50% (MINIMUM_OUTPUT=0 effectively) to 85%.
    /// This test documents and enforces that the new floor catches more
    /// manipulation than the old one.
    #[test]
    fn test_old_50_percent_floor_is_gone() {
        let expected: u64 = 1000;
        let old_floor = expected / 2; // 500 (old 50% floor)
        let new_floor = (expected as u128 * CARNAGE_SLIPPAGE_BPS_ATOMIC as u128 / 10_000) as u64; // 850 (new 85%)
        assert!(
            new_floor > old_floor,
            "New 85% floor must be tighter than old 50%"
        );
    }
}
