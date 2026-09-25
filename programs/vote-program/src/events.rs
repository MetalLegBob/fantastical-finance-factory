//! Vote Program events — the UI/audit surface.
//!
//! `VoteCast` fires once per (voter, week) from `cast_vote` (Plan 02).
//! `WeekFinalized` fires once per finalized week from `finalize_week` (Plan 03).

use anchor_lang::prelude::*;

/// Emitted by `cast_vote`: one voter locked `weight` behind `choice` for
/// `week_index`. One-shot immutable — there is never a second `VoteCast` for the
/// same (voter, week) because the Receipt PDA's existence blocks a re-cast.
#[event]
pub struct VoteCast {
    /// The signing staker whose live `UserStake.staked_balance` was read.
    pub voter: Pubkey,
    /// The week the cast tallies into (casts during week W tally into W).
    pub week_index: u64,
    /// 0 = Burn (Policy A), 1 = LpAdd (Policy B). See `state::POLICY_*`.
    pub choice: u8,
    /// The voter's stake weight, locked at cast time (accepted staleness: a later
    /// unstake does NOT decrement this — both policies are depth-positive).
    pub weight: u64,
}

/// Emitted by `finalize_week`: week `target_week`'s tally was materialized into the
/// `HaulPolicy` singleton governing `effective_week` (= target_week + 1).
#[event]
pub struct WeekFinalized {
    /// The completed week whose tally was read.
    pub target_week: u64,
    /// The week the materialized policy governs (VOTE-03: the FOLLOWING week).
    pub effective_week: u64,
    /// Final Policy-A (Burn) running weight at close.
    pub weight_burn: u128,
    /// Final Policy-B (LpAdd) running weight at close.
    pub weight_lp: u128,
    /// The winning policy recorded: 0 = Burn, 1 = LpAdd.
    pub policy: u8,
    /// true when the deterministic default applied (tie OR zero participation →
    /// LpAdd, VOTE-04) rather than a strict weight win.
    pub default_applied: bool,
}
