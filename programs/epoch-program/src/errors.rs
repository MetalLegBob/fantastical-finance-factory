//! Epoch Program error codes.
//!
//! Source: Epoch_State_Machine_Spec.md Section 11

use anchor_lang::prelude::*;

#[error_code]
pub enum EpochError {
    /// Epoch state has already been initialized
    #[msg("Epoch state already initialized")]
    AlreadyInitialized,

    /// Epoch state has not been initialized
    #[msg("Epoch state not initialized")]
    NotInitialized,

    /// Invalid epoch state (corrupted or unexpected data)
    #[msg("Invalid epoch state")]
    InvalidEpochState,

    /// Epoch boundary has not been reached yet
    #[msg("Epoch boundary has not been reached yet")]
    EpochBoundaryNotReached,

    /// VRF request is already pending
    #[msg("VRF request is already pending")]
    VrfAlreadyPending,

    /// No VRF request is pending
    #[msg("No VRF request is pending")]
    NoVrfPending,

    /// Randomness account data could not be parsed
    #[msg("Randomness account data could not be parsed")]
    RandomnessParseError,

    /// Randomness account is stale (seed_slot too old)
    #[msg("Randomness account is stale (seed_slot too old)")]
    RandomnessExpired,

    /// Randomness has already been revealed (cannot commit)
    #[msg("Randomness has already been revealed (cannot commit)")]
    RandomnessAlreadyRevealed,

    /// Randomness account does not match committed account
    #[msg("Randomness account does not match committed account")]
    RandomnessAccountMismatch,

    /// Randomness has not been revealed by oracle yet
    #[msg("Randomness has not been revealed by oracle yet")]
    RandomnessNotRevealed,

    /// Insufficient randomness bytes (need 8)
    #[msg("Insufficient randomness bytes (need 8)")]
    InsufficientRandomness,

    /// VRF timeout has not elapsed (wait 300 slots)
    #[msg("VRF timeout has not elapsed (wait 300 slots)")]
    VrfTimeoutNotElapsed,

    /// No Carnage execution is pending
    #[msg("No Carnage execution is pending")]
    NoCarnagePending,

    /// Carnage execution deadline has expired
    #[msg("Carnage execution deadline has expired")]
    CarnageDeadlineExpired,

    /// Carnage deadline has not expired yet
    #[msg("Carnage deadline has not expired yet")]
    CarnageDeadlineNotExpired,

    /// Carnage lock window is still active (only atomic path allowed)
    #[msg("Carnage lock window active (atomic-only period)")]
    CarnageLockActive,

    /// Invalid Carnage target pool
    #[msg("Invalid Carnage target pool")]
    InvalidCarnageTargetPool,

    // =========================================================================
    // Carnage Fund-specific errors (Source: Carnage_Fund_Spec.md Section 15)
    // =========================================================================
    /// Carnage fund not initialized
    #[msg("Carnage fund not initialized")]
    CarnageNotInitialized,

    /// Carnage fund already initialized
    #[msg("Carnage fund already initialized")]
    CarnageAlreadyInitialized,

    /// Insufficient SOL in Carnage vault
    #[msg("Insufficient SOL in Carnage vault")]
    InsufficientCarnageSol,

    /// Carnage swap execution failed
    #[msg("Carnage swap execution failed")]
    CarnageSwapFailed,

    /// Carnage burn execution failed
    #[msg("Carnage burn execution failed")]
    CarnageBurnFailed,

    /// Arithmetic overflow
    #[msg("Arithmetic overflow")]
    Overflow,

    /// Insufficient SOL in treasury for bounty
    #[msg("Insufficient SOL in treasury for bounty")]
    InsufficientTreasuryBalance,

    /// Randomness account not owned by the ORAO VRF program
    #[msg("Randomness account not owned by the ORAO VRF program")]
    InvalidRandomnessOwner,

    /// Carnage WSOL account not owned by CarnageSigner PDA
    #[msg("Carnage WSOL account not owned by CarnageSigner PDA")]
    InvalidCarnageWsolOwner,

    /// Staking program address does not match expected program ID
    #[msg("Staking program address mismatch")]
    InvalidStakingProgram,

    /// Invalid mint account (doesn't match expected vault mint)
    #[msg("Invalid mint account")]
    InvalidMint,

    /// Carnage swap received too few tokens (slippage exceeded)
    #[msg("Carnage swap slippage exceeded (below minimum output floor)")]
    CarnageSlippageExceeded,

    /// Tax program address does not match expected program ID
    #[msg("Tax program address mismatch")]
    InvalidTaxProgram,

    /// AMM program address does not match expected program ID
    #[msg("AMM program address mismatch")]
    InvalidAmmProgram,

    /// Invalid cheap_side value stored in EpochState
    #[msg("Invalid cheap_side value -- expected 0 (CRIME) or 1 (FRAUD)")]
    InvalidCheapSide,

    /// Configured epoch length is outside the allowed range
    #[msg("Epoch length is outside the allowed range")]
    EpochLengthOutOfBounds,

    /// Configured post-transition pause exceeds the maximum
    #[msg("Pause slots exceed the allowed maximum")]
    PauseSlotsExceedsMax,

    /// Configured post-transition pause must be shorter than the epoch
    #[msg("Pause slots must be below the epoch length")]
    PauseSlotsNotBelowEpochLength,

    /// A required ArbConfig role was set to the default pubkey
    #[msg("Zero pubkeys are forbidden for ArbConfig roles")]
    ZeroPubkeyForbidden,

    /// ArbConfig roles overlap outside the wallet_a == wallet_b exception
    #[msg("ArbConfig role overlap is forbidden")]
    RoleOverlapForbidden,

    /// Signer is not the current ArbConfig authority
    #[msg("Unauthorized ArbConfig authority")]
    UnauthorizedArbConfigAuthority,

    /// No ArbConfig authority transfer is pending
    #[msg("No ArbConfig authority transfer is pending")]
    NoPendingAuthority,

    /// Signer is not the pending ArbConfig authority
    #[msg("Unauthorized pending ArbConfig authority")]
    UnauthorizedPendingAuthority,

    /// Signer cannot trigger an epoch transition before the grace boundary
    #[msg("Unauthorized epoch transition trigger")]
    UnauthorizedTrigger,

    /// Signer cannot set the manual trading pause
    #[msg("Unauthorized trading pause setter")]
    UnauthorizedPauseSet,

    /// Signer cannot clear the manual trading pause
    #[msg("Unauthorized trading pause clearer")]
    UnauthorizedPauseClear,

    /// Retained from the previous oracle integration and intentionally NOT
    /// removed. Under
    /// ORAO there is no caller-controlled queue dimension, so nothing throws
    /// this any more. The variant stays because the keeper classifies Epoch
    /// failures by NUMERIC code and 178/179 evidence cites 6044/6045/6054/6057;
    /// a dead variant costs nothing, a shifted code silently breaks live error
    /// handling. Never delete, reorder or renumber -- append only.
    #[msg("Randomness account belongs to an unapproved oracle queue")]
    RandomnessQueueMismatch,

    /// The reserved-byte safety carve has not been explicitly activated.
    #[msg("Epoch safety state must be activated before transitions")]
    SafetyActivationRequired,

    /// The one-way safety activation was already completed.
    #[msg("Epoch safety state is already activated")]
    SafetyAlreadyActivated,

    /// Migration is deterministic only when no old transition/action is live.
    #[msg("Epoch safety activation requires quiescent VRF and Carnage state")]
    SafetyActivationRequiresQuiescent,

    /// The ORAO request account's seed is not the seed this epoch/attempt
    /// derives to, i.e. the commitment changed after it was bound.
    #[msg("Randomness commitment does not match the derived request seed")]
    RandomnessCommitmentMismatch,

    /// No further randomness replacement is allowed for this generation.
    #[msg("VRF attempt limit reached")]
    VrfAttemptLimitReached,

    /// The absolute transition lifetime has elapsed; terminal recovery is required.
    #[msg("VRF absolute pending-age limit reached")]
    VrfAbsoluteAgeLimitReached,

    /// Terminal recovery was requested before either objective limit was met.
    #[msg("VRF terminal fallback limit has not been reached")]
    VrfTerminalLimitNotReached,

    /// A Carnage command was built for a different epoch/generation.
    #[msg("Carnage generation does not match the pending action")]
    CarnageGenerationMismatch,

    /// A prior Carnage action must execute or expire before another transition.
    #[msg("Pending Carnage blocks a new epoch transition")]
    CarnagePendingBlocksTransition,

    /// A previous epoch transition is not yet deterministically resolved.
    #[msg("Previous epoch transition is not confirmed")]
    PreviousTransitionUnconfirmed,

    /// A retry must bind a genuinely new commitment account.
    #[msg("VRF retry randomness account must differ from the pending account")]
    RandomnessReplacementUnchanged,

    /// Once an objective terminal boundary is crossed, only the deterministic
    /// fallback may resolve the generation.
    #[msg("VRF generation requires terminal fallback")]
    VrfTerminalTransitionRequired,

    /// A non-keeper attempted to select a replacement commitment before the
    /// bounded public recovery window opened.
    #[msg("Unauthorized VRF retry before the public recovery window")]
    UnauthorizedVrfRetry,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remediation_error_codes_are_append_only() {
        use anchor_lang::error::ERROR_CODE_OFFSET;

        assert_eq!(
            EpochError::RandomnessQueueMismatch as u32 + ERROR_CODE_OFFSET,
            6044
        );
        assert_eq!(
            EpochError::SafetyActivationRequired as u32 + ERROR_CODE_OFFSET,
            6045
        );
        assert_eq!(
            EpochError::PreviousTransitionUnconfirmed as u32 + ERROR_CODE_OFFSET,
            6054
        );
        assert_eq!(
            EpochError::UnauthorizedVrfRetry as u32 + ERROR_CODE_OFFSET,
            6057
        );
    }
}
