//! Tax Program error codes.
//!
//! Source: Tax_Pool_Logic_Spec.md Section 19

use anchor_lang::prelude::*;

#[error_code]
pub enum TaxError {
    /// Invalid pool type for this operation (e.g., PROFIT pool in SOL swap instruction)
    #[msg("Invalid pool type for this operation")]
    InvalidPoolType,

    /// Tax calculation resulted in arithmetic overflow
    #[msg("Tax calculation overflow")]
    TaxOverflow,

    /// Output amount is less than user's minimum_output parameter
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,

    /// EpochState account is invalid or cannot provide tax rates
    #[msg("Invalid epoch state - cannot determine tax rates")]
    InvalidEpochState,

    /// Input amount is too small for a meaningful swap
    #[msg("Insufficient input amount for swap")]
    InsufficientInput,

    /// Net output after tax is below minimum
    #[msg("Output amount below minimum")]
    OutputBelowMinimum,

    /// The swap_authority PDA derivation is incorrect
    #[msg("Invalid swap authority PDA")]
    InvalidSwapAuthority,

    /// Expected SPL Token program for WSOL operations
    #[msg("Token program mismatch - expected SPL Token for WSOL")]
    WsolProgramMismatch,

    /// Expected Token-2022 program for CRIME/FRAUD/PROFIT operations
    #[msg("Token program mismatch - expected Token-2022 for CRIME/FRAUD/PROFIT")]
    Token2022ProgramMismatch,

    /// Token account owner is not the expected user
    #[msg("Invalid token account owner")]
    InvalidTokenOwner,

    /// Carnage-exempt instruction called by non-Carnage authority
    #[msg("Carnage-only instruction called by non-Carnage authority")]
    UnauthorizedCarnageCall,

    /// Staking escrow PDA does not match expected derivation
    #[msg("Staking escrow PDA mismatch")]
    InvalidStakingEscrow,

    /// Carnage vault PDA does not match expected derivation
    #[msg("Carnage vault PDA mismatch")]
    InvalidCarnageVault,

    /// Treasury address does not match expected pubkey
    #[msg("Treasury address mismatch")]
    InvalidTreasury,

    /// AMM program address does not match expected program ID
    #[msg("AMM program address mismatch")]
    InvalidAmmProgram,

    /// Staking program address does not match expected program ID
    #[msg("Staking program address mismatch")]
    InvalidStakingProgram,

    /// Tax amount equals or exceeds gross swap output.
    /// Reject the sell -- net output would be zero or negative.
    #[msg("Tax exceeds gross output -- sell amount too small")]
    InsufficientOutput,

    /// User's minimum_amount_out is below the protocol-enforced floor.
    /// The floor is 50% of constant-product expected output.
    /// Set minimum_amount_out to at least the floor value.
    ///
    /// Source: Phase 49 (SEC-10) -- prevents zero-slippage sandwich attacks
    #[msg("Minimum output below protocol floor (50% of expected)")]
    MinimumOutputFloorViolation,

    /// Pool account is not owned by AMM program.
    /// Prevents spoofed pool accounts from feeding arbitrary reserve data
    /// to swap calculations and slippage floor enforcement.
    ///
    /// Source: Phase 80 (DEF-01) -- pool account ownership verification
    #[msg("Pool account is not owned by AMM program")]
    InvalidPoolOwner,

    /// Post-transition epoch pause window is active (Clock slot < pause_end_slot).
    /// Phase 167 (TAX-01) — spec §4.1 names this variant; frontend FE-04 matches on it. Code 6019.
    #[msg("Trading paused until the post-transition pause window ends")]
    EpochPauseActive,

    /// Manual protocol trading pause engaged by the pause authority (spec §14.1-7 / §14.3-3).
    /// Checked BEFORE the epoch pause — the operator override is the louder signal. Code 6020.
    #[msg("Trading is paused by the protocol")]
    TradingPauseActive,

    /// SOL buy lane requires a WSOL-quote pool: pool.mint_a must be the native mint
    /// (§14.1 ruling 8; also rejects reversed-WSOL pools the hardcoded-AtoB lane cannot trade). Code 6021.
    #[msg("Pool is not a WSOL-quote pool (mint_a must be WSOL)")]
    NonWsolQuotePool,

    /// SPL and arb swap lanes require exactly one CRIME or FRAUD pool side. Code 6022.
    #[msg("Pool has no single faction side (CRIME/FRAUD)")]
    NotAFactionPool,

    /// Arb-wallet swap, sweep-withdrawal, and swept-distribution lanes require a valid ArbConfig. Code 6023.
    #[msg("Invalid ArbConfig account")]
    InvalidArbConfig,

    /// Arb-wallet swap, sweep-withdrawal, and swept-distribution lanes admit only ArbConfig wallets. Code 6024.
    #[msg("Signer is not an authorized ArbConfig wallet")]
    UnauthorizedArbWallet,

    /// The sweep-withdrawal lane pays only an ArbConfig wallet or authority owner. Code 6025.
    #[msg("Sweep destination owner is not allowlisted in ArbConfig")]
    InvalidSweepDestination,

    /// The sweep-withdrawal lane cannot withdraw more than the sweep ATA balance. Code 6026.
    #[msg("Withdrawal amount exceeds the sweep balance")]
    AmountExceedsSweepBalance,

    /// The swept-distribution lane requires an exact nonzero lamport amount. Code 6027.
    #[msg("Swept distribution amount must be greater than zero")]
    ZeroDistribution,

    /// The legacy SOL ABI's faction witness disagrees with the faction derived
    /// from the authenticated AMM pool. The witness is retained for client ABI
    /// compatibility only and never selects the tax schedule. Code 6028.
    #[msg("Faction witness does not match the authenticated pool")]
    FactionWitnessMismatch,

    /// A mint account supplied to Tax does not match the corresponding mint
    /// stored in the authenticated AMM pool. Code 6029.
    #[msg("Supplied mint account does not match the authenticated pool")]
    PoolMintMismatch,

    /// An amount-sensitive path encountered a current or scheduled nonzero
    /// Token-2022 transfer fee. Such assets are outside supported scope. Code 6030.
    #[msg("Current or scheduled Token-2022 transfer fees are unsupported")]
    UnsupportedTransferFee,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the Phase-168 new-surface errors to their append-only on-chain codes.
    #[test]
    fn test_new_surface_error_codes() {
        use anchor_lang::error::ERROR_CODE_OFFSET;

        assert_eq!(TaxError::NotAFactionPool as u32 + ERROR_CODE_OFFSET, 6022);
        assert_eq!(TaxError::InvalidArbConfig as u32 + ERROR_CODE_OFFSET, 6023);
        assert_eq!(
            TaxError::UnauthorizedArbWallet as u32 + ERROR_CODE_OFFSET,
            6024
        );
        assert_eq!(
            TaxError::InvalidSweepDestination as u32 + ERROR_CODE_OFFSET,
            6025
        );
        assert_eq!(
            TaxError::AmountExceedsSweepBalance as u32 + ERROR_CODE_OFFSET,
            6026
        );
        assert_eq!(TaxError::ZeroDistribution as u32 + ERROR_CODE_OFFSET, 6027);
        assert_eq!(
            TaxError::FactionWitnessMismatch as u32 + ERROR_CODE_OFFSET,
            6028
        );
        assert_eq!(TaxError::PoolMintMismatch as u32 + ERROR_CODE_OFFSET, 6029);
        assert_eq!(
            TaxError::UnsupportedTransferFee as u32 + ERROR_CODE_OFFSET,
            6030
        );
    }
}
