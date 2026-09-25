use anchor_lang::prelude::*;

#[error_code]
pub enum AmmError {
    // --- Phase 8: Swap math errors ---
    /// Arithmetic overflow in swap calculation.
    /// The math module returns None on overflow; this error is the on-chain mapping.
    #[msg("Arithmetic overflow in swap calculation")]
    Overflow,

    /// The k-invariant (reserve_a * reserve_b) decreased after swap.
    /// This indicates a bug in swap logic -- k must never decrease.
    #[msg("K-invariant violation: k decreased after swap")]
    KInvariantViolation,

    // --- Phase 9: Pool initialization errors ---
    /// Pool has already been initialized with liquidity.
    /// Each pool PDA can only be initialized once.
    #[msg("Pool is already initialized")]
    PoolAlreadyInitialized,

    /// Mints must be provided in canonical order (mint_a < mint_b by pubkey bytes).
    /// This ensures a unique PDA per unordered mint pair.
    #[msg("Mints must be in canonical order (mint_a < mint_b)")]
    MintsNotCanonicallyOrdered,

    /// The signer is not the admin stored in AdminConfig.
    #[msg("Unauthorized: signer is not the admin")]
    Unauthorized,

    /// A token program account does not match the owner of its corresponding mint.
    /// Each mint's on-chain owner must equal the token program passed for that side.
    #[msg("Token program does not match mint owner")]
    InvalidTokenProgram,

    /// Initial seed amounts must be greater than zero on both sides.
    /// Zero-liquidity pools would cause division-by-zero in swap math.
    #[msg("Initial seed amount must be greater than zero")]
    ZeroSeedAmount,

    /// Mint A and Mint B must be different tokens.
    /// A pool pairing a token with itself has no economic purpose.
    #[msg("Mint A and Mint B must be different")]
    DuplicateMints,

    // --- Phase 10: Transfer routing errors ---
    /// Transfer amount must be greater than zero.
    /// Zero-amount transfers waste compute and indicate a logic bug.
    #[msg("Transfer amount must be greater than zero")]
    ZeroAmount,

    // --- Phase 11: Swap errors ---
    /// Computed output amount is less than the user's minimum_amount_out.
    /// Triggers when price has moved unfavorably beyond the user's tolerance.
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,

    /// Pool has not been initialized with liquidity yet.
    /// Swaps cannot execute on an uninitialized pool (reserves are zero).
    #[msg("Pool is not initialized")]
    PoolNotInitialized,

    /// Pool reentrancy guard is active.
    /// A swap is already in progress; concurrent access is rejected.
    #[msg("Pool is locked")]
    PoolLocked,

    /// Vault account does not match the vault stored in pool state.
    /// Prevents substituting a fake vault to steal funds.
    #[msg("Vault does not match pool state")]
    VaultMismatch,

    /// Mint account does not match the mint stored in pool state.
    /// Prevents swapping the wrong token through a pool.
    #[msg("Mint does not match pool state")]
    InvalidMint,

    // --- Phase 12: Zero-output swap errors ---
    /// Swap fee deduction produced zero effective input.
    /// Input amount is too small for the fee rate -- all tokens would be taken as fee.
    #[msg("Input amount too small: fee deduction produces zero effective input")]
    ZeroEffectiveInput,

    /// Swap math produced zero output tokens.
    /// Effective input is too small relative to reserves to produce any output.
    #[msg("Swap produces zero output tokens")]
    ZeroSwapOutput,

    // --- Phase 13: Access control errors ---
    /// Swap instructions require swap_authority PDA signed by Tax Program.
    /// Direct calls without valid swap_authority are not allowed.
    #[msg("Swaps must go through Tax Program - direct calls not allowed")]
    InvalidSwapAuthority,

    // --- Phase 37: LP fee cap ---
    /// LP fee exceeds maximum allowed (500 bps = 5%).
    /// Prevents admin misconfiguration that could drain liquidity.
    #[msg("LP fee exceeds maximum allowed (500 bps)")]
    LpFeeExceedsMax,

    // --- Phase 97: Transfer authority errors ---
    /// New authority cannot be Pubkey::default() (use burn_admin for that).
    /// This prevents accidental authority burn via the transfer instruction.
    #[msg("Invalid authority: cannot transfer to Pubkey::default()")]
    InvalidAuthority,

    // --- Phase 169: protocol-owned liquidity errors (APPEND-ONLY below) ---
    /// The AMM-computed matching quote side exceeds the caller's budget.
    #[msg("Computed quote side exceeds the max_quote budget bound")]
    ExceedsMaxQuote,

    /// The add would be one-sided because an input or computed side is zero.
    #[msg("Liquidity add would be one-sided")]
    ZeroLiquidity,

    /// The pool does not contain exactly one CRIME or FRAUD mint side.
    #[msg("Pool must contain exactly one faction mint")]
    NotFactionPool,

    /// The ArbConfig account is unreadable or not owned by the Epoch Program.
    #[msg("Invalid ArbConfig account")]
    InvalidArbConfig,

    /// Percentage removals must leave some protocol liquidity in the pool.
    #[msg("Invalid share: share_bps must be within [1, 9999]")]
    InvalidShare,

    /// A percentage removal would leave fewer than 1,000 raw units on a side.
    #[msg("Liquidity removal would breach the 1,000-unit reserve dust floor")]
    DustFloorBreached,

    /// The payout owner is not an ArbConfig wallet or authority.
    #[msg("Destination owner is not allowlisted in ArbConfig")]
    InvalidDestination,

    /// Nominal reserve accounting supports only amount-preserving mints.
    /// Token-2022 mints with a current or scheduled nonzero transfer fee are
    /// rejected before any reserve mutation or token movement.
    #[msg("Current or scheduled Token-2022 transfer fees are unsupported")]
    UnsupportedTransferFee,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the Phase-169 add-liquidity error codes by their on-chain numbers.
    #[test]
    fn test_add_liquidity_error_codes() {
        use anchor_lang::error::ERROR_CODE_OFFSET;

        assert_eq!(AmmError::ExceedsMaxQuote as u32 + ERROR_CODE_OFFSET, 6019);
        assert_eq!(AmmError::ZeroLiquidity as u32 + ERROR_CODE_OFFSET, 6020);
        assert_eq!(AmmError::NotFactionPool as u32 + ERROR_CODE_OFFSET, 6021);
        assert_eq!(AmmError::InvalidArbConfig as u32 + ERROR_CODE_OFFSET, 6022);
        assert_eq!(AmmError::InvalidShare as u32 + ERROR_CODE_OFFSET, 6023);
        assert_eq!(AmmError::DustFloorBreached as u32 + ERROR_CODE_OFFSET, 6024);
        assert_eq!(
            AmmError::InvalidDestination as u32 + ERROR_CODE_OFFSET,
            6025
        );
        assert_eq!(
            AmmError::UnsupportedTransferFee as u32 + ERROR_CODE_OFFSET,
            6026
        );
    }
}
