//! Vote Program error codes.
//!
//! ORDER IS STABLE — codes are 6000 + index. Plan 02/03 may APPEND new variants;
//! NEVER reorder or insert (that would renumber every code below the insertion and
//! desync debug logs / client decoders).

use anchor_lang::prelude::*;

#[error_code]
pub enum VoteError {
    /// 6000 — `current_week()` guard: the vote system has no weeks before genesis.
    #[msg("timestamp precedes vote genesis")]
    BeforeGenesis,

    /// 6001 — the passed `user_stake` account is too short to carry
    /// `staked_balance` at offset 40, or its 8-byte discriminator is not
    /// `UserStake` (manual cross-program decode guard, Plan 02).
    #[msg("UserStake account too short or undecodable")]
    BadUserStake,

    /// 6002 — `UserStake.owner` (bytes 8..40) != the signing voter. Belt+braces on
    /// top of the `seeds::program` canonical-PDA derivation: you cannot cast with
    /// someone else's stake position.
    #[msg("UserStake.owner != voter")]
    StakeOwnerMismatch,

    /// 6003 — `staked_balance == 0`. Eligibility is ANY positive staked balance
    /// (VOTE-02); zero stake carries zero weight and cannot cast.
    #[msg("voter has zero staked balance")]
    NoStake,

    /// 6004 — used by `cast_vote` (Plan 02): the caller-passed `week_index` arg does
    /// not equal the live current week. Casts are for the CURRENT week only.
    /// DISTINCT from `WeekNotEnded` (6005) so debug logs are unambiguous:
    /// 6004 = cast-arg mismatch, 6005 = finalize-before-week-elapsed.
    #[msg("passed week_index != the live current week — cast is for the current week only")]
    WeekMismatch,

    /// 6005 — used by `finalize_week` (Plan 03): the target week has not ended yet
    /// (`current_week <= target_week`). Finalize only materializes COMPLETED weeks.
    #[msg("target week has not ended yet")]
    WeekNotEnded,

    /// 6006 — `finalize_week` idempotency guard: `last_finalized_week >=
    /// target_week` — the week is already materialized; never a re-write.
    #[msg("week already finalized — idempotent no-op")]
    AlreadyFinalized,

    /// 6007 — vote choice must be one of the two policies: 0 = Burn, 1 = LpAdd.
    #[msg("vote choice must be 0=Burn or 1=LpAdd")]
    InvalidChoice,

    /// 6008 — a tally counter `checked_add` overflowed (APPENDED in Plan 02).
    /// Unreachable with real PROFIT supply (u128 counters summing u64 stakes —
    /// Pitfall 7's overflow-proof choice); kept as house overflow discipline.
    #[msg("tally weight addition overflowed")]
    MathOverflow,
}
