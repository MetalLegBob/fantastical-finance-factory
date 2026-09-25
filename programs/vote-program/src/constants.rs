//! Vote Program constants.
//!
//! The cluster-gated cross-program refs (the staking id split — THE Phase-161
//! landmine), the week arithmetic (genesis anchor + cluster-gated week length),
//! the PDA seed literals, and the pinned `UserStake` discriminator/offset the
//! Plan-02 manual cross-program read decodes against.

use crate::errors::VoteError;
use anchor_lang::prelude::*;

// ---------------------------------------------------------------------------
// Cross-Program Ref: the Staking program (owner of the UserStake accounts)
// ---------------------------------------------------------------------------
//
// SPEC §14.10/.11 CLUSTER SPLIT: this crate now carries cfg-resolved declare_id!
// arms in source. The staking REF helper below remains cfg-resolved at COMPILE
// TIME as well, so each build embeds the staking id for its selected cluster.
//
// LOUD NOTE for the deploy pipeline: this is a cross-program REF helper (like
// the conversion-vault mints), resolved by the `devnet` cargo feature — it is
// NOT patched by scripts/deploy/sync-program-ids.ts. Do NOT register it there
// (the 155-04 "sync-only 27/29" fragility is exactly what this avoids).
//
// Plan 02's cast_vote uses this for BOTH guards on the foreign UserStake:
//   owner = staking_program_id()  AND  seeds::program = staking_program_id()

/// Staking program id — devnet arm from the Phase-173 frozen identity graph.
/// LiteSVM tests never compile this arm — they install fixtures at the
/// canonical id below.
#[cfg(feature = "devnet")]
pub fn staking_program_id() -> Pubkey {
    pubkey!("3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC")
}

/// Staking program id — canonical arm: matches
/// `programs/epoch-program/src/constants.rs::staking_program_id()` and
/// `programs/staking/src/lib.rs::declare_id!`. Serves mainnet builds AND
/// LiteSVM/localnet tests (which install their UserStake fixtures at this id).
#[cfg(not(feature = "devnet"))]
pub fn staking_program_id() -> Pubkey {
    pubkey!("12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH")
}

// ---------------------------------------------------------------------------
// Week Arithmetic (VOTE-03: weekly Mon→Sun UTC cadence)
// ---------------------------------------------------------------------------

/// The vote-week genesis anchor: 2024-01-01 00:00:00 UTC — a REAL past Monday
/// (provenance: `date -u -r 1704067200` → "Mon Jan  1 00:00:00 UTC 2024",
/// re-verified 2026-07-18). IDENTICAL on all clusters. Because genesis is a
/// Monday and the mainnet week is exactly 604_800s, every mainnet week boundary
/// (GENESIS + k*WEEK_SECONDS) lands on a Monday 00:00 UTC — the VOTE-03 Mon→Sun
/// cadence by construction. Devnet short-weeks intentionally do NOT align to
/// calendar Mondays; they exercise the same rollover code path faster.
pub const GENESIS_MONDAY_00_UTC: i64 = 1_704_067_200;

/// Cluster-gated week length — devnet arm: a SHORT 1-hour week so the live
/// cast→finalize→rollover drill (Plan 04) runs inside the phase. Same code
/// path as mainnet; only this constant differs (PM devnet-timing precedent).
#[cfg(feature = "devnet")]
pub const WEEK_SECONDS: i64 = 3_600;

/// Cluster-gated week length — canonical arm: a fixed real week (7 * 86_400).
/// ALSO the value LiteSVM tests compile and clock-warp across, so the suite
/// proves the REAL mainnet constant (the refund_clock_test DEADLINE_SLOTS
/// idiom).
#[cfg(not(feature = "devnet"))]
pub const WEEK_SECONDS: i64 = 604_800;

/// Map a unix timestamp to its vote-week bucket:
/// `week_index = (unix_ts − GENESIS_MONDAY_00_UTC) / WEEK_SECONDS`.
///
/// Pre-genesis timestamps are rejected (`VoteError::BeforeGenesis`) — the
/// subtraction is therefore non-negative and the `as u64` cast is lossless.
/// Casts during week W tally into week W; the winner governs week W+1
/// (`finalize_week` writes `effective_week = target_week + 1`, Plan 03).
pub fn current_week(unix_ts: i64) -> Result<u64> {
    require!(unix_ts >= GENESIS_MONDAY_00_UTC, VoteError::BeforeGenesis);
    Ok(((unix_ts - GENESIS_MONDAY_00_UTC) / WEEK_SECONDS) as u64)
}

// ---------------------------------------------------------------------------
// PDA Seeds
// ---------------------------------------------------------------------------

/// Per-week tally: seeds = [TALLY_SEED, week_index.to_le_bytes()]. RETAINED
/// forever (never closed) for audit/UI history.
pub const TALLY_SEED: &[u8] = b"tally";

/// Per-voter-per-week receipt: seeds = [RECEIPT_SEED, week_index.to_le_bytes(),
/// voter]. Its EXISTENCE is the one-vote-per-week guard (plain `init` in
/// Plan 02 — never `init_if_needed`).
pub const RECEIPT_SEED: &[u8] = b"receipt";

/// The HaulPolicy singleton: seeds = [HAUL_POLICY_SEED]. Overwritten each
/// finalize; read per-fire by the Phase-162 disposition reader.
pub const HAUL_POLICY_SEED: &[u8] = b"haul_policy";

/// The STAKING program's per-user stake seed — matches
/// `programs/staking/src/state/user_stake.rs` ("user_stake", user). Used with
/// `seeds::program = staking_program_id()` to re-derive the canonical foreign
/// PDA (an attacker cannot substitute a bigger stranger's stake).
pub const USER_STAKE_SEED: &[u8] = b"user_stake";

// ---------------------------------------------------------------------------
// Foreign UserStake layout pins (manual cross-program decode, Plan 02)
// ---------------------------------------------------------------------------
//
// PINNED-OFFSET PRECEDENT (the epoch_state transition-flags idiom): the vote
// program reads the staking program's 105-byte UserStake account MANUALLY
// (AccountInfo + guards — typed Account<UserStake> is a cluster-owner-collision
// trap, see 161-RESEARCH Pitfall 2). These two pins are the decode contract:
//
//   bytes 0..8   = discriminator                  (tamper/type guard)
//   bytes 8..40  = owner: Pubkey                  (belt+braces == voter check)
//   bytes 40..48 = staked_balance: u64 LE         (the vote weight, VOTE-02)
//
// Verified LIVE on devnet 2026-07-17 (a real UserStake account decode) AND
// against programs/staking/src/state/user_stake.rs. The `user_stake_disc_matches`
// unit test below recomputes the discriminator from first principles so any
// drift fails at `cargo test`, not on-chain.

/// sha256("account:UserStake")[..8] — the Anchor account discriminator of the
/// staking program's UserStake.
pub const USER_STAKE_DISCRIMINATOR: [u8; 8] = [0x66, 0x35, 0xa3, 0x6b, 0x09, 0x8a, 0x57, 0x99];

/// Byte offset of `staked_balance: u64` inside UserStake data
/// (8 discriminator + 32 owner = 40).
pub const USER_STAKE_STAKED_BALANCE_OFFSET: usize = 40;

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The 158-01 "single literal + test" agreement: the non-devnet build
    /// (tests, mainnet) MUST pin the canonical staking id — a drift fails here,
    /// not on-chain. Mirrors epoch-program's test_staking_program_id.
    #[cfg(not(feature = "devnet"))]
    #[test]
    fn staking_id_pins_canonical() {
        assert_eq!(
            staking_program_id().to_string(),
            "12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH"
        );
    }

    /// The devnet arm's pin — run via `cargo test -p vote-program --lib
    /// --features devnet`. Asserts the devnet arm equals the id recorded in
    /// deployments/devnet.json (the ONLY source of truth for devnet ids —
    /// "resolve from deployments/devnet.json, never recall"). Deliberately id-
    /// literal-free so the devnet id appears exactly ONCE in this file (the arm).
    #[cfg(feature = "devnet")]
    #[test]
    fn staking_id_pins_fresh_manifest() {
        let devnet_json =
            include_str!("../../../.planning/phases/173-devnet-mirror-0b/173-IDENTITIES.json");
        let id = staking_program_id().to_string();
        assert!(
            devnet_json.contains(&format!("\"staking\": \"{}\"", id)),
            "devnet arm id {} not found as .programs.staking in deployments/devnet.json",
            id
        );
    }

    /// Recompute sha256("account:UserStake")[..8] from first principles and
    /// assert the pinned constant matches (cheap tamper/type-guard drift check).
    #[test]
    fn user_stake_disc_matches() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"account:UserStake");
        let hash = hasher.finalize();
        assert_eq!(
            USER_STAKE_DISCRIMINATOR[..],
            hash[..8],
            "USER_STAKE_DISCRIMINATOR drifted from sha256(\"account:UserStake\")[..8]"
        );
    }

    /// Week arithmetic at the exact boundaries, expressed in terms of the
    /// COMPILED WEEK_SECONDS arm (tests compile the non-devnet 604_800 arm; the
    /// same assertions hold for the devnet arm under --features devnet).
    #[test]
    fn week_boundaries() {
        // The compiled arm carries the expected cluster value.
        #[cfg(not(feature = "devnet"))]
        assert_eq!(WEEK_SECONDS, 604_800, "canonical arm = 1 real week");
        #[cfg(feature = "devnet")]
        assert_eq!(WEEK_SECONDS, 3_600, "devnet arm = 1-hour drill week");

        // Genesis instant is week 0.
        assert_eq!(current_week(GENESIS_MONDAY_00_UTC).unwrap(), 0);
        // Last second of week 0.
        assert_eq!(
            current_week(GENESIS_MONDAY_00_UTC + WEEK_SECONDS - 1).unwrap(),
            0
        );
        // First second of week 1.
        assert_eq!(
            current_week(GENESIS_MONDAY_00_UTC + WEEK_SECONDS).unwrap(),
            1
        );
        // Pre-genesis is rejected with BeforeGenesis.
        let err = current_week(GENESIS_MONDAY_00_UTC - 1).unwrap_err();
        assert_eq!(err, VoteError::BeforeGenesis.into());
    }

    /// The genesis anchor is the verified Monday (see the constant's
    /// provenance comment) — pin the raw value so a silent edit fails loudly.
    #[test]
    fn genesis_pins_verified_monday() {
        assert_eq!(GENESIS_MONDAY_00_UTC, 1_704_067_200);
    }

    /// Seed literals are the exact bytes the PDA derivations use.
    #[test]
    fn seed_literals_pinned() {
        assert_eq!(TALLY_SEED, b"tally");
        assert_eq!(RECEIPT_SEED, b"receipt");
        assert_eq!(HAUL_POLICY_SEED, b"haul_policy");
        // Must byte-match the staking program's own seed (user_stake.rs).
        assert_eq!(USER_STAKE_SEED, b"user_stake");
        assert_eq!(USER_STAKE_STAKED_BALANCE_OFFSET, 40);
    }
}
