//! Vote Program account shapes: `Tally`, `Receipt`, `HaulPolicy`.
//!
//! Sizing MECHANISM (solana-anchor-claude-skill MANDATE): every struct derives
//! its space — account size is ALWAYS `8 + <Struct>::INIT_SPACE`. NO hand-rolled
//! LEN consts, NO magic size numbers. The pinned byte layouts documented below
//! are the READ contract (for the Phase-162 disposition reader + frontend), not
//! a sizing source; the `serialized offsets` unit tests keep docs and reality
//! locked together (the epoch_state transition-flags precedent).
//!
//! Anti-gaming note (accepted staleness, 161-RESEARCH Pitfall 8): a cast locks
//! choice AND weight one-shot; unstaking later does NOT decrement recorded
//! weight. Safe because BOTH policies are depth-positive — gaming the vote
//! cannot produce a depth-negative outcome. Do NOT add snapshots or stake-age
//! gates.

use anchor_lang::prelude::*;

// ---------------------------------------------------------------------------
// Policy / choice encoding (shared by Receipt.choice and HaulPolicy.policy)
// ---------------------------------------------------------------------------

/// Policy A — buy-and-burn week. The on-chain `HaulPolicy` records the POLICY
/// CHOICE only; disposal execution and custody live in the bot per BOT-05.
pub const POLICY_BURN: u8 = 0;

/// Policy B — LP-add week. The bot executes LP-add across the v2.0 pool set
/// using per-pool P&L attribution (BOT-05); on-chain `HaulPolicy` records the
/// POLICY CHOICE only. ALSO the deterministic DEFAULT: ties and
/// zero-participation weeks resolve to LpAdd (VOTE-04), and an off-chain reader
/// falls back to LpAdd when `HaulPolicy` is stale — fail-to-default.
pub const POLICY_LP_ADD: u8 = 1;

// ---------------------------------------------------------------------------
// Tally — per-week running counters (O(1) close; never iterate voters)
// ---------------------------------------------------------------------------

/// Weekly vote tally. seeds = [TALLY_SEED, week_index.to_le_bytes()].
///
/// Created by the FIRST caster of the week (`init_if_needed` in Plan 02 —
/// safe: casts only ADD to counters, and Anchor never re-zeros an existing
/// discriminator-checked account; the RE-VOTE guard is the Receipt, never this
/// account). RETAINED forever (never closed) for audit/UI history.
///
/// Counters are u128: overflow-proof sums of u64 stakes (total staked PROFIT
/// fits u64 with room, but u128 makes the sum unconditionally safe — Pitfall 7).
///
/// PINNED LAYOUT (on-chain bytes, 8-byte discriminator included; borsh = fields
/// in declaration order, LE, no padding). Account size = 8 + 73 = 81 bytes.
///   0..8    discriminator = sha256("account:Tally")[..8]
///   8..16   week_index: u64
///   16..32  weight_burn: u128        (Policy A running total)
///   32..48  weight_lp: u128          (Policy B running total)
///   48      bump: u8
///   49..81  reserved: [u8; 32]       (audit/UI headroom, zeroed)
#[account]
#[derive(InitSpace)]
pub struct Tally {
    /// The week these counters belong to (redundant with the PDA seed, kept
    /// in-account for direct decode by UI/audit tooling).
    pub week_index: u64,
    /// Policy A (Burn) running weight total.
    pub weight_burn: u128,
    /// Policy B (LpAdd) running weight total.
    pub weight_lp: u128,
    /// PDA bump, stored for cheap re-derivation.
    pub bump: u8,
    /// Zeroed headroom (audit/UI). Read-as-disabled; no setter anywhere.
    pub reserved: [u8; 32],
}

// ---------------------------------------------------------------------------
// Receipt — the one-vote-per-week guard
// ---------------------------------------------------------------------------

/// Per-voter-per-week vote receipt.
/// seeds = [RECEIPT_SEED, week_index.to_le_bytes(), voter].
///
/// Its EXISTENCE is the one-vote-per-week guard: Plan 02 creates it with plain
/// `init` (NEVER `init_if_needed` — Pitfall 3), so a second cast in the same
/// week fails with "account already in use". One-shot immutable: choice AND
/// weight lock at cast time (accepted staleness — see module docs).
///
/// PINNED LAYOUT (on-chain bytes). Account size = 8 + 50 = 58 bytes.
///   0..8    discriminator = sha256("account:Receipt")[..8]
///   8..40   voter: Pubkey
///   40..48  week_index: u64
///   48      choice: u8               (POLICY_BURN | POLICY_LP_ADD)
///   49..57  weight: u64              (locked at cast; individual stake fits u64)
///   57      bump: u8
#[account]
#[derive(InitSpace)]
pub struct Receipt {
    /// The staker who cast (signs the cast TX; == UserStake.owner).
    pub voter: Pubkey,
    /// The week this cast tallied into.
    pub week_index: u64,
    /// The locked choice: POLICY_BURN (0) or POLICY_LP_ADD (1).
    pub choice: u8,
    /// The locked weight = live UserStake.staked_balance at cast time (VOTE-02).
    pub weight: u64,
    /// PDA bump, stored for cheap re-derivation.
    pub bump: u8,
}

// ---------------------------------------------------------------------------
// HaulPolicy — the singleton the off-chain bot and frontend readers consume
// ---------------------------------------------------------------------------

/// THE policy singleton. seeds = [HAUL_POLICY_SEED]. Overwritten by each
/// `finalize_week`; its readers are now OFF-CHAIN: the bot seed module under
/// `scripts/vote/` and the Phase-175 frontend.
///
/// VOTE-05 (QUORUM ROOM — read this before touching `reserved`): the reserved
/// bytes are room for a FUTURE quorum / participation-incentive design and are
/// read-as-disabled. There is NO setter instruction, NO admin account, NO
/// config surface anywhere in this program. Activating a quorum later is a
/// PROGRAM-UPGRADE decision taken from observed participation data — never a
/// runtime knob.
///
/// VOTE-08 execution boundary: THIS program creates NO holding account and
/// moves NO funds. Disposal execution and custody live in the bot per BOT-05;
/// `HaulPolicy` records only the policy choice that the bot applies.
///
/// OFF-CHAIN READER STALENESS CONTRACT (bot seed module `scripts/vote/`;
/// Phase-175 FE): if `effective_week <` the reader's current week (finalize
/// never ran), the reader falls back to the LP-add default (fail-to-default,
/// VOTE-04 spirit). `finalize_week` may materialize the most recent COMPLETED
/// week whenever it is finally called.
///
/// PINNED LAYOUT (on-chain bytes — the 162 reader + FE decode contract; the
/// epoch_state pinned-offset precedent). Account size = 8 + 115 = 123 bytes.
///   0..8    discriminator = sha256("account:HaulPolicy")[..8]
///   8       policy: u8               (POLICY_BURN | POLICY_LP_ADD)
///   9..17   effective_week: u64      (the week this policy governs = target+1)
///   17..25  last_finalized_week: u64 (idempotency high-water mark)
///   25..41  weight_burn: u128        (closing snapshot, audit/UI)
///   41..57  weight_lp: u128          (closing snapshot, audit/UI)
///   57      default_applied: bool    (tie/zero-participation → LpAdd)
///   58      bump: u8
///   59..123 reserved: [u8; 64]       (VOTE-05 quorum room, zeroed, NO setter)
#[account]
#[derive(InitSpace)]
pub struct HaulPolicy {
    /// The winning policy for `effective_week`: POLICY_BURN or POLICY_LP_ADD.
    /// Forward-compatible u8 (room for e.g. Split(x%) variants later).
    pub policy: u8,
    /// The week this policy governs (VOTE-03: the week FOLLOWING the tallied one).
    pub effective_week: u64,
    /// Highest week ever finalized — double-finalize guard (Pitfall 6).
    pub last_finalized_week: u64,
    /// Closing Policy-A weight snapshot (audit/UI).
    pub weight_burn: u128,
    /// Closing Policy-B weight snapshot (audit/UI).
    pub weight_lp: u128,
    /// true when the deterministic LP-add default applied (tie/zero turnout).
    pub default_applied: bool,
    /// PDA bump, stored for cheap re-derivation.
    pub bump: u8,
    /// VOTE-05 quorum room — read-as-disabled, NO setter IX, NO admin surface.
    pub reserved: [u8; 64],
}

// ---------------------------------------------------------------------------
// Unit tests — size pins + the HaulPolicy serialized-offset contract
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the derived sizes so ANY field add/remove/resize fails loudly here
    /// (and forces a deliberate layout-doc + 162-reader-contract update).
    #[test]
    fn init_space_pins() {
        assert_eq!(Tally::INIT_SPACE, 73, "Tally data size drifted");
        assert_eq!(Receipt::INIT_SPACE, 50, "Receipt data size drifted");
        assert_eq!(HaulPolicy::INIT_SPACE, 115, "HaulPolicy data size drifted");
    }

    /// The epoch_state precedent: serialize a recognizable HaulPolicy and
    /// assert the documented byte offsets (DATA offsets — on-chain = +8 for the
    /// discriminator). This is the 162 reader's decode contract.
    #[test]
    fn haul_policy_serialized_offsets() {
        let hp = HaulPolicy {
            policy: 0xAA,
            effective_week: 0x0807060504030201,      // LE: 01..08
            last_finalized_week: 0x100F0E0D0C0B0A09, // LE: 09..10
            weight_burn: u128::from_le_bytes([0x11; 16]),
            weight_lp: u128::from_le_bytes([0x22; 16]),
            default_applied: true,
            bump: 0xFD,
            reserved: [0; 64],
        };
        let mut buf = Vec::new();
        hp.serialize(&mut buf).unwrap();

        assert_eq!(
            buf.len(),
            HaulPolicy::INIT_SPACE,
            "serialized len == derived space"
        );

        // policy at data offset 0 (on-chain 8)
        assert_eq!(buf[0], 0xAA);
        // effective_week at data offset 1..9 (on-chain 9..17)
        assert_eq!(buf[1], 0x01);
        assert_eq!(buf[8], 0x08);
        // last_finalized_week at data offset 9..17 (on-chain 17..25)
        assert_eq!(buf[9], 0x09);
        assert_eq!(buf[16], 0x10);
        // weight_burn at data offset 17..33 (on-chain 25..41)
        assert_eq!(buf[17], 0x11);
        assert_eq!(buf[32], 0x11);
        // weight_lp at data offset 33..49 (on-chain 41..57)
        assert_eq!(buf[33], 0x22);
        assert_eq!(buf[48], 0x22);
        // default_applied at data offset 49 (on-chain 57)
        assert_eq!(buf[49], 1);
        // bump at data offset 50 (on-chain 58)
        assert_eq!(buf[50], 0xFD);
        // reserved (VOTE-05 quorum room) at data offset 51..115, zeroed
        assert!(
            buf[51..115].iter().all(|&b| b == 0),
            "reserved (quorum room) bytes 51..115 must be zeroed"
        );
    }

    /// Tally + Receipt offset spot-checks (same borsh contract).
    #[test]
    fn tally_receipt_serialized_offsets() {
        let t = Tally {
            week_index: 0x0807060504030201,
            weight_burn: u128::from_le_bytes([0x33; 16]),
            weight_lp: u128::from_le_bytes([0x44; 16]),
            bump: 0xFE,
            reserved: [0; 32],
        };
        let mut buf = Vec::new();
        t.serialize(&mut buf).unwrap();
        assert_eq!(buf.len(), Tally::INIT_SPACE);
        assert_eq!(buf[0], 0x01); // week_index @ 0..8
        assert_eq!(buf[8], 0x33); // weight_burn @ 8..24
        assert_eq!(buf[24], 0x44); // weight_lp @ 24..40
        assert_eq!(buf[40], 0xFE); // bump @ 40

        let voter = Pubkey::new_from_array([0x55; 32]);
        let r = Receipt {
            voter,
            week_index: 7,
            choice: POLICY_LP_ADD,
            weight: 0x0807060504030201,
            bump: 0xFC,
        };
        let mut buf = Vec::new();
        r.serialize(&mut buf).unwrap();
        assert_eq!(buf.len(), Receipt::INIT_SPACE);
        assert_eq!(buf[0], 0x55); // voter @ 0..32
        assert_eq!(buf[32], 7); // week_index @ 32..40
        assert_eq!(buf[40], POLICY_LP_ADD); // choice @ 40
        assert_eq!(buf[41], 0x01); // weight @ 41..49
        assert_eq!(buf[49], 0xFC); // bump @ 49
    }

    /// The two-policy encoding is fixed: 0 = Burn, 1 = LpAdd (shared by
    /// Receipt.choice and HaulPolicy.policy; VOTE-04's default is LpAdd).
    #[test]
    fn policy_encoding_pinned() {
        assert_eq!(POLICY_BURN, 0);
        assert_eq!(POLICY_LP_ADD, 1);
    }
}
