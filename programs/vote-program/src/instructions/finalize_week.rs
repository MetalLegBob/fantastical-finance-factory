//! `finalize_week` — the permissionless weekly crank that MATERIALIZES the vote
//! winner into the `HaulPolicy` singleton (VOTE-03/04/05/06/08).
//!
//! Once week `target_week` has fully ENDED, anyone may call this to read
//! `Tally[target_week]`, pick the higher-weight policy (tie/zero-participation
//! → the deterministic LP-add default, VOTE-04), and write
//! `HaulPolicy { effective_week = target_week + 1 }` — the winner governs the
//! FOLLOWING week (VOTE-03). The on-chain program records the policy choice;
//! disposal execution and custody live in the off-chain bot per BOT-05.
//!
//! ## Permissionless + idempotent
//!
//! ANY signer may call — the keeper gets no waiver and no special identity
//! exists (VOTE-05: zero admin surface). Griefing is impossible because the
//! outcome is a pure function of on-chain state (the Tally weights + the
//! clock); WHO calls changes nothing. Idempotency is a high-water mark:
//!
//!   ALLOW iff `effective_week == 0` (virgin state — no real finalize has ever
//!   run, because every real finalize writes `effective_week = target + 1 >= 1`)
//!   OR `target_week > last_finalized_week`.
//!
//! Consequences (all deliberate): a double `finalize_week(W)` reverts
//! `AlreadyFinalized` and NEVER re-writes; week 0 itself is finalizable from
//! the `initialize` defaults (`last_finalized_week = 0` alone would wall it
//! off); and catch-up materializes the MOST RECENT completed week — older
//! skipped weeks stay unmaterialized forever, which is exactly the Phase-162
//! fail-to-default contract (a reader seeing `effective_week` < its current
//! week falls back to the LP-add default).
//!
//! ## The AUDIT-03-safe absent-Tally read
//!
//! `Tally[target_week]` only exists if someone cast that week (the first caster
//! `init_if_needed`s it). A zero-participation week therefore presents an
//! UNINITIALIZED account — and "optional account silently skips critical
//! logic" is exactly the pattern this repo variant-hunts (AUDIT-03). So the
//! tally here is a REQUIRED, seed-constrained `AccountInfo` — it can never be
//! omitted, and the seeds re-derivation means an attacker cannot substitute a
//! different week's (or a forged) account. Existence is decided in the handler
//! by OWNER, and the absent case is an EXPLICIT zero-participation branch that
//! still finalizes (to the LP-add default) — never a skip.
//!
//! ## RECORD-ONLY (VOTE-06 boundary)
//!
//! This instruction materializes the DECISION and nothing else: it moves no
//! funds, creates no accounts, and makes no CPI of any kind. Execution of
//! either policy is Phase 162 (see the write-site comment in the handler).

use anchor_lang::prelude::*;

use crate::constants::{current_week, HAUL_POLICY_SEED, TALLY_SEED};
use crate::errors::VoteError;
use crate::events::WeekFinalized;
use crate::state::{HaulPolicy, Tally, POLICY_BURN, POLICY_LP_ADD};

/// Accounts for `finalize_week(target_week)`.
///
/// Account order: `[cranker, tally, haul_policy]`.
#[derive(Accounts)]
#[instruction(target_week: u64)]
pub struct FinalizeWeek<'info> {
    /// ANY signer — the permissionless crank (VOTE-05: no keeper waiver, no
    /// admin identity; the outcome is a pure function of on-chain state).
    pub cranker: Signer<'info>,

    /// CHECK: seed-constrained to the canonical `Tally[target_week]` PDA
    /// (`bump` with no stored value re-derives the canonical address), so an
    /// attacker can neither substitute another account nor omit this one.
    /// The account MAY be uninitialized — a not-yet-created Tally PDA is still
    /// address-valid; EXISTENCE is checked in the handler by owner (initialized
    /// => decode weights; never created => the explicit zero-participation
    /// branch). Deliberately NOT an omittable optional account (AUDIT-03).
    #[account(seeds = [TALLY_SEED, target_week.to_le_bytes().as_ref()], bump)]
    pub tally: AccountInfo<'info>,

    /// The `HaulPolicy` singleton this crank overwrites. Must already exist —
    /// created once by `initialize` (the typed `Account` check fails loudly on
    /// a missing/foreign account).
    #[account(mut, seeds = [HAUL_POLICY_SEED], bump)]
    pub haul_policy: Account<'info, HaulPolicy>,
}

/// Handler: week-ended guard → idempotency high-water → AUDIT-03-safe
/// participation read → winner/tie/zero resolution → materialize + event.
pub fn handler(ctx: Context<FinalizeWeek>, target_week: u64) -> Result<()> {
    // (a) Only a week that has FULLY ended can be finalized: the live clock
    // must have moved past target_week entirely (current_week > target_week).
    // In-progress and future weeks reject with WeekNotEnded (6005 — distinct
    // from cast_vote's WeekMismatch 6004, so logs are unambiguous).
    let now = Clock::get()?.unix_timestamp;
    require!(current_week(now)? > target_week, VoteError::WeekNotEnded);

    // (b) Idempotency high-water (the predicate documented in the module docs):
    // virgin state (`effective_week == 0` — impossible after any real finalize,
    // which always writes target + 1 >= 1) admits ANY completed week including
    // week 0; afterwards only weeks strictly above the high-water mark pass. A
    // double finalize_week(W) therefore reverts here and HaulPolicy is never
    // re-written for an already-finalized week.
    let haul_policy = &mut ctx.accounts.haul_policy;
    require!(
        haul_policy.effective_week == 0 || target_week > haul_policy.last_finalized_week,
        VoteError::AlreadyFinalized
    );

    // (c) Participation read — the AUDIT-03-safe existence check on the
    // REQUIRED, seed-constrained tally account.
    let tally_info = &ctx.accounts.tally;
    let (weight_burn, weight_lp) = if tally_info.owner == &crate::ID {
        // This program owns the account at the canonical Tally[target_week]
        // PDA => it was initialized by cast_vote (the ONLY writer at this
        // seed; a PDA cannot be `assign`ed to a program from outside because
        // assignment requires the account's own signature, which a PDA cannot
        // produce externally). It MUST therefore decode: try_deserialize
        // enforces the 8-byte discriminator AND the full field length
        // (>= 8 + Tally::INIT_SPACE on-chain), and a failure here is state
        // corruption that reverts LOUD — it is NEVER read as zero
        // participation.
        let data = tally_info.try_borrow_data()?;
        let mut slice: &[u8] = &data;
        let tally = Tally::try_deserialize(&mut slice)?;
        (tally.weight_burn, tally.weight_lp)
    } else {
        // Never created (system-owned / empty data): no cast landed in
        // target_week. This is the EXPLICIT zero-participation branch —
        // finalize still proceeds and materializes the deterministic LP-add
        // default below (VOTE-04). Never a skip.
        (0u128, 0u128)
    };

    // (d) Winner-takes-all with the deterministic default (VOTE-04, no quorum):
    //   weight_burn > weight_lp          => Burn, a strict win;
    //   weight_burn == weight_lp (ties,  => LpAdd with default_applied = true
    //     including 0 == 0 zero turnout)    (the deterministic default);
    //   weight_lp > weight_burn          => LpAdd, a strict win (NOT a default).
    let (policy, default_applied) = if weight_burn > weight_lp {
        (POLICY_BURN, false)
    } else {
        (POLICY_LP_ADD, weight_burn == weight_lp)
    };

    // (e) Materialize the decision into the singleton.
    //
    // RECORD-ONLY. finalize_week materializes the DECISION; it moves NO funds
    // and makes NO burn/LP/token CPI. Disposal execution and custody live in
    // the off-chain bot per BOT-05. Its reader falls back to LpAdd when
    // HaulPolicy.effective_week < its current week (fail-to-default).
    //
    // `target_week + 1` cannot overflow: guard (a) proved
    // target_week < current_week, and current_week is a clock-bounded u64
    // (an i64-seconds range divided by WEEK_SECONDS).
    //
    // `reserved` is deliberately UNTOUCHED (VOTE-05: the quorum room is
    // read-as-disabled; finalize never writes it and no setter exists).
    haul_policy.policy = policy;
    haul_policy.effective_week = target_week + 1;
    haul_policy.last_finalized_week = target_week;
    haul_policy.weight_burn = weight_burn;
    haul_policy.weight_lp = weight_lp;
    haul_policy.default_applied = default_applied;

    // (f) The UI/audit event — one per finalized week, ever (the high-water
    // guard makes a second emission for the same week impossible).
    emit!(WeekFinalized {
        target_week,
        effective_week: target_week + 1,
        weight_burn,
        weight_lp,
        policy,
        default_applied,
    });

    Ok(())
}
