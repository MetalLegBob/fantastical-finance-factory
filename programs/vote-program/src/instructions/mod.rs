//! Vote Program instructions (Phase 161).
//!
//! The COMPLETE, LOCKED instruction set of the vote mechanism (VOTE-05 — no
//! setter/admin/quorum surface exists or will be added at runtime):
//! `initialize` (one-time HaulPolicy singleton with FIXED safe defaults),
//! `cast_vote` (the heart — a stake-weighted, one-shot-immutable vote whose
//! weight is the voter's LIVE `UserStake.staked_balance`, read cross-program
//! from the staking program), and `finalize_week` (the permissionless weekly
//! crank that materializes the winner into `HaulPolicy` for the FOLLOWING
//! week — record-only; execution is Phase 162).

pub mod cast_vote;
pub mod finalize_week;
pub mod initialize;

pub use cast_vote::*;
pub use finalize_week::*;
pub use initialize::*;
