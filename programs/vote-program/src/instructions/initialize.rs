//! `initialize` — one-time permissionless creation of the `HaulPolicy` singleton.
//!
//! Creates the ONE `HaulPolicy` PDA (`seeds = [HAUL_POLICY_SEED]`) with FIXED safe
//! defaults — nothing here is caller-controlled, which is exactly why the IX can be
//! permissionless with zero griefing surface: whoever calls it first (deployer, in
//! practice) merely pays the rent for a state everyone agrees on. A second call
//! fails loudly on the explicit `init` (repo style: explicit one-time init, never
//! the idempotent-init constraint).
//!
//! The defaults ARE the VOTE-04 fail-to-default posture: `policy = POLICY_LP_ADD`
//! with `default_applied = true` and `effective_week = 0` — i.e. "no week has ever
//! been finalized; if anyone asks, the answer is the deterministic LP-add default".
//! The Phase-162 reader's staleness contract (see `state.rs`) resolves to the same
//! answer, so the pre-first-finalize world is consistent from every angle.
//!
//! NO token account is created or touched anywhere in this program (VOTE-08 is
//! satisfied by DESIGNATION: the surplus rests as WSOL in the existing
//! `caller_wsol` — see the `HaulPolicy` docs). NO admin account exists (VOTE-05).

use anchor_lang::prelude::*;

use crate::constants::HAUL_POLICY_SEED;
use crate::state::{HaulPolicy, POLICY_LP_ADD};

/// Accounts for the one-time `initialize`.
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The `HaulPolicy` singleton, created here (explicit one-time `init`: a second
    /// call fails on the already-initialized account — the "creates once" guard).
    #[account(
        init,
        payer = payer,
        space = 8 + HaulPolicy::INIT_SPACE,
        seeds = [HAUL_POLICY_SEED],
        bump,
    )]
    pub haul_policy: Account<'info, HaulPolicy>,

    /// Any signer — funds the singleton's rent. Safe to leave permissionless
    /// because the created state is HARDCODED below (no caller-controlled field).
    #[account(mut)]
    pub payer: Signer<'info>,

    /// System program (account creation).
    pub system_program: Program<'info, System>,
}

/// Handler: write the FIXED safe defaults. No arguments, no caller influence.
pub fn handler(ctx: Context<Initialize>) -> Result<()> {
    let haul_policy = &mut ctx.accounts.haul_policy;

    // The deterministic default policy (VOTE-04): LP-add, flagged as a default
    // (not a voted win). `effective_week = 0` + `last_finalized_week = 0` mean
    // "nothing finalized yet" — the Plan-03 finalize_week high-water guard treats
    // week 0 specially only in that it can still BE finalized once it completes.
    haul_policy.policy = POLICY_LP_ADD;
    haul_policy.effective_week = 0;
    haul_policy.last_finalized_week = 0;
    haul_policy.weight_burn = 0;
    haul_policy.weight_lp = 0;
    haul_policy.default_applied = true;
    haul_policy.bump = ctx.bumps.haul_policy;
    haul_policy.reserved = [0u8; 64]; // VOTE-05 quorum room — zeroed, read-as-disabled

    msg!("HaulPolicy singleton initialized (fixed LP-add default, no admin surface)");
    Ok(())
}
