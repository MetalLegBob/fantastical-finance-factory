//! Dr Fraudsworth v2.0 weekly PROFIT-staker surplus-disposal vote program.
//!
//! Lets PROFIT stakers cast a stake-weighted vote between the two haul policies
//! (Policy A buy-and-burn vs Policy B double-sided LP-add) on a weekly Mon→Sun UTC
//! cadence, materializing the winner into the `HaulPolicy` singleton that governs the
//! FOLLOWING week's haul disposal. This Phase-170 port is pinned to donor commit
//! `8b0a9197`; the on-chain program records the policy choice only, while policy
//! EXECUTION lives in the off-chain bot per BOT-05.
//!
//! The instruction set is COMPLETE and LOCKED at three entries (VOTE-05):
//! `initialize`, `cast_vote`, and permissionless `finalize_week`. Stake weight is
//! read manually from the staking program's `UserStake` account at cast time;
//! this program holds no disposal funds and makes no policy-execution CPI.

// Mirror the in-tree conversion-vault hardening (Phase 149 sos-005): the cluster
// features are mutually exclusive. They select cluster constants, and devnet also
// selects its fresh declaration, so enabling both would create an ambiguous binary.
// Fail loudly at build time.
#[cfg(all(feature = "devnet", feature = "localnet"))]
compile_error!("vote-program: features `devnet` and `localnet` are mutually exclusive — pick one");

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

use instructions::*;

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Dr Fraudsworth's Finance Factory",
    project_url: "https://fraudsworth.fun",
    contacts: "email:drfraudsworth@gmail.com,twitter:@fraudsworth",
    policy: "https://fraudsworth.fun/docs/security/security-policy",
    preferred_languages: "en",
    auditors: "Internal audits: SOS #4, BOK, DB #3 (v1.5+)",
    expiry: "2027-03-20"
}

#[cfg(all(not(feature = "no-entrypoint"), feature = "devnet"))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_VOTE: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=Gua6CZTdvKArTgqC9eTr1TqQ2PZ4d4cUnBqUx8W8W5Dh|staking=3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC";
#[cfg(all(
    not(feature = "no-entrypoint"),
    not(any(feature = "devnet", feature = "localnet"))
))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_VOTE: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=8xeFnfKe6CrzS2MZoP9qVhRCAR1KX8isDpLNuurP9Lsd|staking=12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH";

// Cluster-gated program id — spec §14.1 rulings 10+11 (2026-08-21). The devnet arm
// is a fresh devnet-only id for the Phase-173 mirror. The canonical arm is the public
// pending mainnet ID reserved for Phase 180: Vote has never been live on mainnet, and
// this declaration carries no account, signer, ProgramData, hash, or authority claim.
// A wrong-feature deploy fails loudly with Anchor 4100 instead of silently selecting
// the wrong cluster timing. Pin tests: mod declare_id_pins (bottom of file).
#[cfg(feature = "devnet")]
declare_id!("Gua6CZTdvKArTgqC9eTr1TqQ2PZ4d4cUnBqUx8W8W5Dh");
#[cfg(not(feature = "devnet"))]
declare_id!("8xeFnfKe6CrzS2MZoP9qVhRCAR1KX8isDpLNuurP9Lsd");

/// The Vote Program instruction surface — COMPLETE and LOCKED at exactly these
/// three entries (VOTE-05): there is NO admin surface and NO quorum setter anywhere
/// in this program — quorum room exists only as read-as-disabled reserved bytes in
/// `HaulPolicy`, and activating it later is a program-upgrade decision, never a
/// runtime knob.
#[program]
pub mod vote_program {
    use super::*;

    /// One-time permissionless creation of the `HaulPolicy` singleton with FIXED
    /// safe defaults (policy = LpAdd, `default_applied = true` — the VOTE-04
    /// posture; nothing caller-controlled). A second call fails loudly on the
    /// explicit `init`.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        instructions::initialize::handler(ctx)
    }

    /// Cast a stake-weighted vote for the CURRENT week (`week_index` is validated
    /// against the live clock). Weight = the voter's live `UserStake.staked_balance`
    /// read cross-program at cast time (VOTE-02); one vote per voter per week,
    /// one-shot immutable (the Receipt's plain-`init` existence guard).
    pub fn cast_vote(ctx: Context<CastVote>, week_index: u64, choice: u8) -> Result<()> {
        instructions::cast_vote::handler(ctx, week_index, choice)
    }

    /// Permissionless weekly crank: once week `target_week` has fully ENDED, read
    /// `Tally[target_week]` (an absent Tally = the explicit zero-participation
    /// branch) and MATERIALIZE the winner — or the deterministic LP-add default on
    /// tie/zero turnout (VOTE-04) — into `HaulPolicy`, governing `target_week + 1`
    /// (VOTE-03). Idempotent per week via the `last_finalized_week` high-water
    /// guard. RECORD-ONLY: moves no funds, makes no CPI — execution is Phase 162
    /// (VOTE-06).
    pub fn finalize_week(ctx: Context<FinalizeWeek>, target_week: u64) -> Result<()> {
        instructions::finalize_week::handler(ctx, target_week)
    }
}

#[cfg(test)]
mod declare_id_pins {
    // Parity guards in the staking_id_pins_* idiom: each cfg arm is pinned so an
    // accidental arm swap fails a unit test, not a devnet deploy (Anchor-4100 class).
    // The devnet arm deliberately has NO deployments/devnet.json binding yet — no
    // vote entry exists there until the Phase-173 mirror deploys and regenerates it;
    // 173 adds the json-binding test then.
    #[cfg(not(feature = "devnet"))]
    #[test]
    fn declare_id_pins_canonical_mainnet() {
        assert_eq!(
            crate::ID.to_string(),
            "8xeFnfKe6CrzS2MZoP9qVhRCAR1KX8isDpLNuurP9Lsd"
        );
    }

    #[cfg(feature = "devnet")]
    #[test]
    fn declare_id_pins_devnet_arm() {
        assert_eq!(
            crate::ID.to_string(),
            "Gua6CZTdvKArTgqC9eTr1TqQ2PZ4d4cUnBqUx8W8W5Dh"
        );
    }
}
