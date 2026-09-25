//! execute_carnage instruction (fallback).
//!
//! Permissionless fallback Carnage execution within the 300-slot deadline window.
//! Called when atomic execution in consume_randomness fails.
//!
//! This instruction performs the same execution logic as execute_carnage_atomic
//! but can be called by anyone AFTER the 50-slot atomic lock window expires
//! and BEFORE the 300-slot deadline. Fallback cannot execute during the
//! 50-slot atomic-only window (CarnageLockActive error).
//!
//! The SOL->WSOL wrap calls (system_program::transfer + sync_native) execute
//! BEFORE the swap at CPI depth 0, so they do NOT impact the swap depth chain.
//!
//! Source: Carnage_Fund_Spec.md Section 13.3, Phase 47 CONTEXT.md

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{TokenAccount, TokenInterface};

use crate::constants::{
    amm_program_id, tax_program_id, CARNAGE_FUND_SEED, CARNAGE_SIGNER_SEED,
    CARNAGE_SLIPPAGE_BPS_FALLBACK, CARNAGE_SOL_VAULT_SEED, EPOCH_SAFETY_VERSION, EPOCH_STATE_SEED,
};
use crate::errors::EpochError;
use crate::helpers::carnage_execution::{execute_carnage_core, CarnageAccounts};
use crate::state::{CarnageFundState, EpochState};

/// Accounts for execute_carnage instruction.
///
/// This instruction executes pending Carnage within the 300-slot deadline window.
/// Permissionless - anyone can call to complete the execution.
/// Fallback cannot execute during the 50-slot atomic lock window.
///
/// STACK BUDGET: State accounts are Box'd to move deserialized data to heap (~247 bytes saved).
/// Pool vaults and mints are AccountInfo because Epoch only forwards them to Tax::swap_exempt CPI --
/// it never reads their data. Tax Program validates them. This avoids ~494 bytes of unnecessary
/// deserialization and keeps the instruction well within the 4096-byte BPF stack frame limit.
///
/// Source: Carnage_Fund_Spec.md Section 13.3, Phase 47 CONTEXT.md
#[derive(Accounts)]
pub struct ExecuteCarnage<'info> {
    /// Caller (anyone - permissionless)
    pub caller: Signer<'info>,

    /// Global epoch state (has pending Carnage flags)
    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
        constraint = epoch_state.safety_version == EPOCH_SAFETY_VERSION
            @ EpochError::SafetyActivationRequired,
        constraint = epoch_state.carnage_pending @ EpochError::NoCarnagePending,
    )]
    pub epoch_state: Box<Account<'info, EpochState>>,

    /// Carnage Fund state (updated after execution)
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

/// Handler for execute_carnage instruction (fallback path).
///
/// Executes pending Carnage within the deadline window after the lock
/// window has expired. Same execution logic as atomic, but with:
/// - 75% slippage floor (more lenient than atomic's 85%)
/// - Deadline + lock window validation
/// - CarnageExecuted event with atomic=false
///
/// Source: Carnage_Fund_Spec.md Section 13.3
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteCarnage<'info>>,
    generation: u32,
) -> Result<()> {
    let clock = Clock::get()?;

    require!(
        generation == ctx.accounts.epoch_state.current_epoch
            && generation == ctx.accounts.epoch_state.carnage_generation,
        EpochError::CarnageGenerationMismatch
    );

    // Validate deadline hasn't expired FIRST (before any execution)
    require!(
        clock.slot <= ctx.accounts.epoch_state.carnage_deadline_slot,
        EpochError::CarnageDeadlineExpired
    );

    // Validate lock window has expired (atomic-only period is over).
    // During the lock window (0 to CARNAGE_LOCK_SLOTS after trigger),
    // only the atomic path bundled with consume_randomness can execute.
    // This prevents MEV bots from front-running the atomic execution.
    require!(
        clock.slot > ctx.accounts.epoch_state.carnage_lock_slot,
        EpochError::CarnageLockActive
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
        CARNAGE_SLIPPAGE_BPS_FALLBACK,
        false, // fallback execution
    )
}

#[cfg(test)]
mod tests {
    use crate::constants::CARNAGE_SLIPPAGE_BPS_FALLBACK;
    use crate::state::CarnageAction;

    #[test]
    fn test_carnage_action_none_value() {
        // Verify CarnageAction::None converts to 0 for clearing
        assert_eq!(CarnageAction::None.to_u8(), 0);
    }

    /// Verify fallback slippage floor: expected=1000, 75% floor = 750.
    #[test]
    fn test_fallback_slippage_floor() {
        let expected: u64 = 1000;
        let min_output = (expected as u128)
            .checked_mul(CARNAGE_SLIPPAGE_BPS_FALLBACK as u128)
            .and_then(|n| n.checked_div(10_000))
            .unwrap() as u64;
        assert_eq!(min_output, 750);
    }

    /// Verify fallback 75% is more lenient than atomic 85%.
    /// This is by design: fallback runs after the lock window expires
    /// and should prioritize execution over optimal pricing.
    #[test]
    fn test_fallback_more_lenient_than_atomic() {
        let expected: u64 = 1000;
        let atomic_floor = (expected as u128 * 8500 / 10_000) as u64;
        let fallback_floor =
            (expected as u128 * CARNAGE_SLIPPAGE_BPS_FALLBACK as u128 / 10_000) as u64;
        assert!(
            fallback_floor < atomic_floor,
            "Fallback 75% must be more lenient than atomic 85%"
        );
    }

    /// Verify the lock window timing logic that governs when fallback
    /// can execute. Simulates three scenarios:
    /// 1. During lock (current_slot <= lock_slot): rejected
    /// 2. After lock, before deadline: allowed (fallback window)
    /// 3. After deadline: expired
    #[test]
    fn test_lock_window_check_logic() {
        // Simulate: lock_slot = 1050, deadline_slot = 1300
        let lock_slot: u64 = 1050;
        let deadline_slot: u64 = 1300;

        // During lock (slot 1025): should be rejected
        let current_slot: u64 = 1025;
        assert!(
            !(current_slot > lock_slot),
            "Should be rejected during lock"
        );

        // After lock, before deadline (slot 1100): should be allowed
        let current_slot: u64 = 1100;
        assert!(current_slot > lock_slot, "Should be allowed after lock");
        assert!(
            current_slot <= deadline_slot,
            "Should still be within deadline"
        );

        // After deadline (slot 1301): should be expired
        let current_slot: u64 = 1301;
        assert!(
            !(current_slot <= deadline_slot),
            "Should be expired after deadline"
        );
    }
}
