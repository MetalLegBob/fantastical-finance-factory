//! `cast_vote` — the heart of the vote mechanism (VOTE-01 / VOTE-02).
//!
//! A PROFIT staker casts a stake-weighted vote for the CURRENT week between the
//! two haul policies (0 = Burn, 1 = LpAdd). The weight is the voter's LIVE
//! `UserStake.staked_balance`, read CROSS-PROGRAM from the staking program at
//! cast time — no snapshot, no stake-age gate, immediate participation.
//!
//! ## The manual cross-program read (the phase's one real research decision)
//!
//! `user_stake` is a bare `AccountInfo` validated by THREE independent Anchor
//! constraints plus two handler guards — NOT a typed `Account<UserStake>`. The
//! typed form bakes an un-overridable `owner == the staking crate's canonical
//! declare_id` check into `try_from`, which collides with the devnet-deployed
//! staking id (the Phase-161 landmine, 161-RESEARCH Pitfall 1/2). The manual
//! form's ONLY owner check is the explicit cluster-correct `staking_program_id()`
//! cfg arm, so the cluster split disappears at compile time.
//!
//! Guard stack (all load-bearing — see the account struct + handler):
//!   1. `seeds` + `seeds::program`  — the account IS the canonical
//!      `["user_stake", voter]` PDA under the staking program (an attacker
//!      cannot substitute a bigger stranger's stake);
//!   2. `owner = staking_program_id()` — the account is really owned by the
//!      staking program (no look-alike from another program);
//!   3. discriminator == `UserStake`  — it's really a UserStake, not another
//!      staking-owned account shape;
//!
//! plus: `voter` SIGNS, and the in-data owner field (bytes 8..40) must equal
//! the voter (belt-and-braces on top of the seed derivation).
//!
//! ## One-shot immutability + ACCEPTED staleness (161-RESEARCH Pitfall 8)
//!
//! The `receipt` uses plain `init`, so a second cast in the same week fails with
//! "already in use" — that EXISTENCE check is the one-vote-per-week guard, and it
//! makes the first cast's choice+weight immutable. Deliberately accepted
//! staleness: unstaking AFTER casting does NOT decrement the recorded weight
//! (weight is live at cast time only). This is safe because BOTH policies are
//! depth-positive — gaming the vote cannot produce a depth-negative outcome. Do
//! NOT add snapshots or stake-age gates.

use anchor_lang::prelude::*;

use crate::constants::{
    current_week, staking_program_id, RECEIPT_SEED, TALLY_SEED, USER_STAKE_DISCRIMINATOR,
    USER_STAKE_SEED, USER_STAKE_STAKED_BALANCE_OFFSET,
};
use crate::errors::VoteError;
use crate::events::VoteCast;
use crate::state::{Receipt, Tally, POLICY_BURN, POLICY_LP_ADD};

/// Accounts for `cast_vote(week_index, choice)`.
///
/// Account order: `[voter, user_stake, tally, receipt, system_program]`.
#[derive(Accounts)]
#[instruction(week_index: u64, choice: u8)]
pub struct CastVote<'info> {
    /// The casting staker. MUST sign — combined with the `seeds` re-derivation on
    /// `user_stake` below, a signer can only ever vote their OWN stake position.
    /// `mut`: pays the Receipt rent (and the Tally rent if first caster of the week).
    #[account(mut)]
    pub voter: Signer<'info>,

    /// The staking program's `UserStake` PDA for `voter`, read CROSS-PROGRAM.
    ///
    /// CHECK: validated by `owner` + `seeds` + `seeds::program` (guards 1+2 in the
    /// module docs); `staked_balance` is decoded manually in the handler behind the
    /// discriminator guard (guard 3) and the in-data owner-field check. Never a
    /// typed foreign `Account<..>` — see the module docs for why.
    #[account(
        owner = staking_program_id(),
        seeds = [USER_STAKE_SEED, voter.key().as_ref()],
        bump,
        seeds::program = staking_program_id(),
    )]
    pub user_stake: AccountInfo<'info>,

    /// The per-week running tally. Created by the FIRST caster of the week (who
    /// pays its rent); every later cast only ADDs to the counters — the
    /// idempotent-init constraint never re-zeros an existing discriminator-checked
    /// account (161-RESEARCH Pitfall 3), and the re-vote guard is the Receipt
    /// below, never this account.
    #[account(
        init_if_needed,
        payer = voter,
        space = 8 + Tally::INIT_SPACE,
        seeds = [TALLY_SEED, week_index.to_le_bytes().as_ref()],
        bump,
    )]
    pub tally: Account<'info, Tally>,

    /// THE one-vote-per-week guard: plain `init` (NEVER the idempotent form —
    /// Pitfall 3), so a second cast by the same voter in the same week fails with
    /// "account already in use" before any handler logic runs. Existence = voted.
    #[account(
        init,
        payer = voter,
        space = 8 + Receipt::INIT_SPACE,
        seeds = [RECEIPT_SEED, week_index.to_le_bytes().as_ref(), voter.key().as_ref()],
        bump,
    )]
    pub receipt: Account<'info, Receipt>,

    /// System program (Tally/Receipt creation).
    pub system_program: Program<'info, System>,
}

/// Handler: validate choice + week, decode the live stake weight cross-program,
/// add it to the chosen counter, and lock the immutable receipt.
pub fn handler(ctx: Context<CastVote>, week_index: u64, choice: u8) -> Result<()> {
    // (a) The choice must be one of the two policies (0 = Burn, 1 = LpAdd).
    require!(
        choice == POLICY_BURN || choice == POLICY_LP_ADD,
        VoteError::InvalidChoice
    );

    // (b) Casts land in the CURRENT week only: the caller-passed `week_index`
    // (which seeded the Tally/Receipt PDAs above) is validated against the live
    // clock, so cross-week forgery is impossible. WeekMismatch (6004) is DISTINCT
    // from finalize_week's WeekNotEnded (6005) so a mismatched-arg cast is
    // unambiguous in logs.
    let now = Clock::get()?.unix_timestamp;
    require!(week_index == current_week(now)?, VoteError::WeekMismatch);

    // (c) The manual cross-program UserStake read (guard 3 + belt-and-braces).
    // Scoped so the data borrow drops before any account mutation below.
    let weight = {
        let data = ctx.accounts.user_stake.try_borrow_data()?;
        // Must carry staked_balance in full: offset 40 + 8 = 48 bytes minimum.
        require!(
            data.len() >= USER_STAKE_STAKED_BALANCE_OFFSET + 8,
            VoteError::BadUserStake
        );
        // Guard 3: really a UserStake (sha256("account:UserStake")[..8]).
        require!(
            data[0..8] == USER_STAKE_DISCRIMINATOR,
            VoteError::BadUserStake
        );
        // Belt-and-braces: the in-data owner field must be the signing voter
        // (the seeds derivation already forces this account to be the voter's
        // own PDA; this catches a corrupted/forged owner field regardless).
        require!(
            data[8..40] == ctx.accounts.voter.key().to_bytes(),
            VoteError::StakeOwnerMismatch
        );
        u64::from_le_bytes(
            data[USER_STAKE_STAKED_BALANCE_OFFSET..USER_STAKE_STAKED_BALANCE_OFFSET + 8]
                .try_into()
                .map_err(|_| VoteError::BadUserStake)?,
        )
    };

    // Eligibility = ANY positive staked balance (VOTE-02): no minimum, no
    // snapshot, no stake-age gate. Zero stake carries zero weight and cannot cast.
    require!(weight > 0, VoteError::NoStake);

    // (d) Tally identity — set idempotently. Safe unconditionally: the PDA seeds
    // bind this account to exactly this week_index, and the canonical bump is a
    // pure function of the address, so a re-set writes the same values.
    let tally = &mut ctx.accounts.tally;
    tally.week_index = week_index;
    tally.bump = ctx.bumps.tally;

    // (e) Add the live weight to the chosen counter (u128 add of a u64 — checked
    // anyway, house overflow discipline).
    if choice == POLICY_BURN {
        tally.weight_burn = tally
            .weight_burn
            .checked_add(weight as u128)
            .ok_or(VoteError::MathOverflow)?;
    } else {
        tally.weight_lp = tally
            .weight_lp
            .checked_add(weight as u128)
            .ok_or(VoteError::MathOverflow)?;
    }

    // (f) Lock the immutable receipt (one-shot: choice AND weight, per Pitfall 8).
    let receipt = &mut ctx.accounts.receipt;
    receipt.voter = ctx.accounts.voter.key();
    receipt.week_index = week_index;
    receipt.choice = choice;
    receipt.weight = weight;
    receipt.bump = ctx.bumps.receipt;

    // (g) The UI/audit event — one per (voter, week), ever.
    emit!(VoteCast {
        voter: ctx.accounts.voter.key(),
        week_index,
        choice,
        weight,
    });

    Ok(())
}
