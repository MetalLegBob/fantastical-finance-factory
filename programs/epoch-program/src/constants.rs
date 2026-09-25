//! Epoch Program constants.
//!
//! Timing parameters and seeds for the Epoch State Machine.
//! Source: Epoch_State_Machine_Spec.md Section 3.1

use anchor_lang::prelude::Pubkey;
use anchor_lang::pubkey;

// ---------------------------------------------------------------------------
// Cross-Program ID Constants
// ---------------------------------------------------------------------------

/// Tax Program ID for address constraint validation.
///
/// Matches declare_id! in tax-program/src/lib.rs.
/// Source keypair: keypairs/tax-program-keypair.json
#[cfg(feature = "devnet")]
pub fn tax_program_id() -> Pubkey {
    pubkey!("5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ")
}

#[cfg(not(feature = "devnet"))]
pub fn tax_program_id() -> Pubkey {
    pubkey!("43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj")
}

/// AMM Program ID for address constraint validation.
///
/// Matches declare_id! in amm/src/lib.rs.
/// Source keypair: keypairs/amm-keypair.json
#[cfg(feature = "devnet")]
pub fn amm_program_id() -> Pubkey {
    pubkey!("AG7QjhAoUecWK8iDDtzB4RzQY2HA7RgHbPeZimJSGwU1")
}

#[cfg(not(feature = "devnet"))]
pub fn amm_program_id() -> Pubkey {
    pubkey!("5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR")
}

/// Staking Program ID for address constraint validation.
///
/// Matches declare_id! in staking/src/lib.rs.
/// Source keypair: keypairs/staking-keypair.json
#[cfg(feature = "devnet")]
pub fn staking_program_id() -> Pubkey {
    pubkey!("3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC")
}

#[cfg(not(feature = "devnet"))]
pub fn staking_program_id() -> Pubkey {
    pubkey!("12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH")
}

// ---------------------------------------------------------------------------
// ORAO VRF v2 Program ID and network-state trust root
// ---------------------------------------------------------------------------
//
// BOTH cfg arms carry the SAME literal, and that is DELIBERATE.
//
// ORAO's program ID and its derived `network_state` PDA are identical on
// devnet and mainnet (verified on-chain 2026-09-19). The only cluster-varying
// ORAO values -- `request_fee` and the fulfilment-authority list -- live
// inside `network_state` and are read from chain at runtime by ORAO's own
// constraints; neither is ever compiled into this binary.
//
// The owner ruled (179.1-CONTEXT.md amendment A-02) to KEEP the cfg-gate
// scaffold anyway. Do NOT "simplify" these into a single constant:
//  * it costs nothing,
//  * it preserves the fail-safe-toward-mainnet pattern used by every other
//    cluster constant in this file (a forgotten `--features devnet` must
//    resolve to the MAINNET arm, never the other way round), and
//  * it prevents a future ORAO cluster split from silently bypassing the gate.

/// ORAO VRF program ID (feature-flagged for devnet/mainnet; see A-02 above).
///
/// Used as the `owner` constraint on the randomness account and as the
/// derivation program for the request PDA, which together prevent
/// fake-randomness injection: a PDA under ORAO's program can only be written
/// by ORAO.
#[cfg(feature = "devnet")]
pub const ORAO_VRF_PROGRAM_ID: Pubkey = pubkey!("VRFzZoJdhFWL8rkvu87LpKM3RbcVezpMEc6X5GVDr7y");

#[cfg(not(feature = "devnet"))]
pub const ORAO_VRF_PROGRAM_ID: Pubkey = pubkey!("VRFzZoJdhFWL8rkvu87LpKM3RbcVezpMEc6X5GVDr7y");

/// ORAO's network-configuration PDA -- the trust root that names the
/// fulfilment-authority set ORAO checks signatures against before XOR-ing.
/// This is the structural analogue of the retired oracle-queue pin.
///
/// Equals `orao_solana_vrf::network_state_account_address(&ORAO_VRF_PROGRAM_ID)`.
/// That helper calls `Pubkey::find_program_address`, which is NOT a `const fn`,
/// so the equality cannot be asserted at compile time the way the program-ID
/// equality below is. It is asserted instead in
/// `tests::orao_network_state_matches_the_derived_pda`, which fails the build
/// of the test profile if the literal ever drifts from the derivation.
#[cfg(feature = "devnet")]
pub const ORAO_NETWORK_STATE: Pubkey = pubkey!("5ER1oENnV4srxYdAynUfRzWeQCPQaqMiAp4VqyMbSqnK");

#[cfg(not(feature = "devnet"))]
pub const ORAO_NETWORK_STATE: Pubkey = pubkey!("5ER1oENnV4srxYdAynUfRzWeQCPQaqMiAp4VqyMbSqnK");

const fn pubkeys_equal(a: &Pubkey, b: &Pubkey) -> bool {
    let a = a.to_bytes();
    let b = b.to_bytes();
    let mut index = 0;
    while index < 32 {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

// A dependency upgrade must never move the oracle program ID silently.
// `ID_CONST` is the const form of the crate's `declare_id!`; `ID` itself is a
// `static` and cannot be read from a const context.
const _: () = assert!(pubkeys_equal(
    &ORAO_VRF_PROGRAM_ID,
    &orao_solana_vrf::ID_CONST
));
// The network-state literal is derived, not chosen. Its equality with
// `network_state_account_address(&ORAO_VRF_PROGRAM_ID)` is asserted in the
// unit tests below (see the doc comment on ORAO_NETWORK_STATE for why it
// cannot be a compile-time assert).
const _: () = assert!(!pubkeys_equal(&ORAO_VRF_PROGRAM_ID, &ORAO_NETWORK_STATE));

// ---------------------------------------------------------------------------
// Timing Parameters
// ---------------------------------------------------------------------------

/// Documented epoch-length initialization reference (~5 minutes devnet,
/// ~30 minutes mainnet). Runtime due checks live-read ArbConfig instead.
/// Source: Centralized-Arb MVP Spec §3.2 and §14.6 ruling L6.
#[cfg(feature = "devnet")]
pub const SLOTS_PER_EPOCH: u64 = 750;

#[cfg(not(feature = "devnet"))]
pub const SLOTS_PER_EPOCH: u64 = 4_500;

/// Maximum configurable post-transition pause duration in slots.
/// Source: Centralized-Arb MVP Spec §3.1 and §14.1 ruling 1.
pub const MAX_PAUSE_SLOTS: u64 = 300;

/// Minimum configurable epoch length in slots.
/// Source: Centralized-Arb MVP Spec §3.1 and §14.1 ruling 1.
pub const MIN_EPOCH_LENGTH_SLOTS: u64 = 150;

/// Maximum configurable epoch length in slots.
/// Source: Centralized-Arb MVP Spec §3.1 and §14.1 ruling 1.
pub const MAX_EPOCH_LENGTH_SLOTS: u64 = 216_000;

/// Overdue window after which trigger_epoch_transition becomes permissionless.
/// A stranger is admitted when overdue >= GRACE_SLOTS.
/// Source: Centralized-Arb MVP Spec §3.2 and §14.6 ruling L2.
pub const GRACE_SLOTS: u64 = 300;

/// Milliseconds per slot estimate for UI display.
/// Conservative estimate accounting for validator variance.
pub const MS_PER_SLOT_ESTIMATE: u64 = 420;

/// VRF timeout in slots (~2 minutes).
/// If oracle doesn't reveal within this window, retry is permitted.
/// Source: Epoch_State_Machine_Spec.md Section 3.1
pub const VRF_TIMEOUT_SLOTS: u64 = 300;

/// Total commitments allowed for one epoch transition, including the initial
/// commitment. A value of three permits at most two replacements.
pub const MAX_VRF_ATTEMPTS: u8 = 3;

/// Absolute lifetime of one pending epoch transition. Retries never move this
/// boundary because `vrf_transition_start_slot` is immutable for a generation.
pub const MAX_VRF_PENDING_SLOTS: u64 = 1_200;

/// Version of the reserved-byte carve used by the bounded VRF/Carnage state
/// machine. Existing singleton accounts must run `activate_epoch_safety`
/// while quiescent before any transition operation is admitted.
pub const EPOCH_SAFETY_VERSION: u8 = 1;

const _: () = assert!(MAX_VRF_ATTEMPTS > 0);
const _: () = assert!(
    MAX_VRF_PENDING_SLOTS > VRF_TIMEOUT_SLOTS * MAX_VRF_ATTEMPTS as u64,
    "absolute VRF age must leave every configured attempt a reveal window"
);

/// Carnage execution deadline in slots (~2 minutes).
/// Total window: 0-50 = atomic-only lock, 50-300 = fallback allowed, >300 = expired.
/// Source: Phase 47 CONTEXT.md
pub const CARNAGE_DEADLINE_SLOTS: u64 = 300;

/// Bounty paid to epoch trigger caller (0.001 SOL).
/// Incentivizes timely epoch transitions.
/// ~66x actual 3-TX base cost -- generous but treasury-efficient.
/// Source: Phase 50 CONTEXT.md
pub const TRIGGER_BOUNTY_LAMPORTS: u64 = 1_000_000;

/// Seed for deriving the EpochState PDA.
/// Single global account: seeds = ["epoch_state"]
/// Source: Epoch_State_Machine_Spec.md Section 4.4
pub const EPOCH_STATE_SEED: &[u8] = b"epoch_state";

/// Seed for deriving the ArbConfig PDA.
/// Single global account: seeds = ["arb_config"]
/// Source: Centralized-Arb MVP Spec §3.1.
pub const ARB_CONFIG_SEED: &[u8] = b"arb_config";

/// Seed for deriving the Carnage signer PDA.
/// Used by Epoch Program to sign CPI calls to Tax Program.
/// Must match Tax Program's CARNAGE_SIGNER_SEED.
pub const CARNAGE_SIGNER_SEED: &[u8] = b"carnage_signer";

// ---------------------------------------------------------------------------
// Tax Rate Constants (Genesis)
// ---------------------------------------------------------------------------

/// Genesis low tax rate in basis points (3%).
/// Source: Epoch_State_Machine_Spec.md Section 5
pub const GENESIS_LOW_TAX_BPS: u16 = 300;

/// Genesis high tax rate in basis points (14%).
/// Source: Epoch_State_Machine_Spec.md Section 5
pub const GENESIS_HIGH_TAX_BPS: u16 = 1400;

// ---------------------------------------------------------------------------
// Staking CPI Constants
// ---------------------------------------------------------------------------

/// Seed for deriving the Staking authority PDA.
/// Used by Epoch Program to sign CPI calls to Staking Program.
/// CRITICAL: Must match Stub Staking's expected seed for seeds::program verification.
pub const STAKING_AUTHORITY_SEED: &[u8] = b"staking_authority";

/// Anchor discriminator for Staking::update_cumulative instruction.
/// Computed: sha256("global:update_cumulative")[0..8]
/// Used when building CPI instruction data.
pub const UPDATE_CUMULATIVE_DISCRIMINATOR: [u8; 8] =
    [0x93, 0x84, 0xdb, 0x65, 0xa5, 0x17, 0x3d, 0x71];

// ---------------------------------------------------------------------------
// Carnage VRF Constants
// ---------------------------------------------------------------------------

/// Carnage slippage floor for atomic path (85% = 8500 bps).
/// Actual output must be >= 85% of constant-product expected output.
/// 15% tolerance covers normal same-TX deviations; MEV defense is primarily atomicity + VRF unpredictability.
/// Source: Phase 47 CONTEXT.md
pub const CARNAGE_SLIPPAGE_BPS_ATOMIC: u64 = 8500;

/// Carnage slippage floor for fallback path (75% = 7500 bps).
/// More lenient than atomic -- prioritize execution over optimal price in recovery mode.
/// Source: Phase 47 CONTEXT.md
pub const CARNAGE_SLIPPAGE_BPS_FALLBACK: u64 = 7500;

/// Lock window in slots during which only the atomic Carnage path can execute.
/// After this window expires (but before CARNAGE_DEADLINE_SLOTS), the fallback path becomes callable.
/// ~20 seconds at 400ms/slot. Gives atomic TX ample time to confirm.
/// Source: Phase 47 CONTEXT.md
pub const CARNAGE_LOCK_SLOTS: u64 = 50;

/// Carnage trigger threshold (byte 5 < 11 triggers, ~4.3% probability).
/// Source: Carnage_Fund_Spec.md Section 7.1
pub const CARNAGE_TRIGGER_THRESHOLD: u8 = 11;

/// Carnage sell action threshold (byte 6 < 5 = sell, 2% probability).
/// Source: Carnage_Fund_Spec.md Section 7.2
pub const CARNAGE_SELL_THRESHOLD: u8 = 5;

/// The AMM's LP fee on a SOL pool, in basis points.
///
/// MIRRORS `programs/amm/src/constants.rs::SOL_POOL_FEE_BPS` (100 = 1.0%). Duplicated
/// rather than imported because the epoch program does not depend on the AMM crate;
/// `friction_bps_matches_the_amm_fee_constant` in `tests/test_carnage_cap.rs` pins the
/// two together so they cannot drift.
///
/// A sandwicher pays it TWICE (once on the front-run buy, once on the back-run sell),
/// which is why the friction below adds `2 * AMM_FEE_BPS`.
pub const AMM_FEE_BPS: u16 = 100;

/// # 🔑 THE ONE CANONICAL EXPLANATION OF THE NS-1 CAP AND ITS DIVISOR `k`.
///
/// Every other site that mentions the carnage cap points HERE rather than restating the
/// algebra, because a second copy is a second thing that can drift -- and a drifted copy
/// of *this* text drove two independent 163.5 audit passes to file false CRITICALs
/// (`DEC-01a` / AI-1, ruled by mlbob 2026-08-13). Do not restate the law elsewhere.
///
/// ## The law: a constant-product buy moves price by `~2D/R`, not `D/R`
///
/// A SOL-denominated buy of size `D` into reserves `(R_sol, R_token)` moves the spot
/// price by a FACTOR of `(1 + D/R)^2`, i.e. a fractional impact of
/// `(1 + D/R)^2 - 1 ~= 2*D/R` -- **TWICE** the naive linear `D/R`. Solving the sandwich
/// properly (front-run `B`, gross profit `B*D*(2R + B + D) / ((R+B)^2 + B*D)`, set equal
/// to the round-trip friction cost `f*B`, attacker's best case `B -> 0`) gives the EXACT
/// break-even size:
///
/// ```text
///     D* = R * (sqrt(1+f) - 1)
/// ```
///
/// `f*R` (k=1) is therefore ~2x TOO PERMISSIVE: simulated at live CRIME depth it allows a
/// **+21.7 SOL** sandwich at f=14% (+32.5 at f=20%) -- a live exploit.
///
/// ## Why `k = 2` was WRONG, and it is not a tuning slip
///
/// 🚨 `k = 2` was **NOT** the zero-EV point and **NOT** a margin under it. It sat
/// **ABOVE** it, permanently. `f/2` is only the FIRST-ORDER Taylor expansion of
/// `sqrt(1+f) - 1`:
///
/// ```text
///     sqrt(1+f) - 1  =  f/2 - f^2/8 + ...
/// ```
///
/// The next term is NEGATIVE, so `f/2` **always overshoots** by ~`f^2/8`. **No value of
/// `f` makes k = 2 land under break-even.** Measured against the exact point:
/// **+3.39% OVER at f=14%, +4.77% OVER at f=20%**. `k` must be >= **2.0954** merely to
/// TOUCH break-even at f=20%, so k=2.05 covers neither end and k=2.10 is razor thin.
///
/// (An earlier draft of this comment, of `carnage_execution.rs` and of NS-1 `:174` said
/// *"k = 2 IS the zero-EV point"*. True only to first order, stated absolutely, and it is
/// the sentence that propagated. NS-1 `:217` -- *"the cap sits 3.4-4.8% in size above the
/// exact point"* -- is the correct one.)
///
/// ## What is shipped: `k = 54/25 = 2.16` (mlbob, DEC-01, Disposition Sitting #1 2026-08-13)
///
/// | k | vs exact break-even @ f=14% | @ f=20% |
/// |---|---|---|
/// | 2.00 (was shipped) | **+3.39% OVER** | **+4.77% OVER** |
/// | **2.16 (shipped)** | **-4.27%** | **-2.99%** |
///
/// Cost: a **7.4% reduction in carnage buy size** (2/2.16). `k = 4` was rejected in NS-1
/// §4 for *halving* carnage, per the standing ruling *"if it can buy big we want it to buy
/// big"*. The exact closed form `D* = isqrt(R^2*(10000+f)/10000) - R` is now ELIGIBLE
/// (`isqrt_u256`'s F-1 obligation proved SUCCESSFUL in 6.18 s, 163.6) and was
/// **REJECTED** anyway: it would plant a walling `isqrt_u256` proof obligation inside
/// live-fired, 164-bound code and add BPF compute to a path whose budget is unmeasured.
///
/// `k` is a rational because a `u64` cannot hold 2.16. See [`CARNAGE_CAP_K_NUM`] /
/// [`CARNAGE_CAP_K_DEN`] and the `const _` below, which makes the zero-EV property a
/// BUILD ERROR to break rather than a review item.
///
/// ## The accepted residual (`SOS-H067`, re-ratified by mlbob 2026-08-13)
///
/// `k` closes only **component (A)** of the self-sandwich residual -- the overshoot itself
/// -- by construction. **Component (B) remains DECLINED**: `reserve_sol` is read **live**,
/// post-front-run (`carnage_execution.rs:336-339`), so a same-transaction front-run
/// inflates the cap's own INPUT and the cap sees `~R+B`. (B) is ~2.9x the size of (A) and
/// is superseded by the planned no-trade-window architecture, which removes the
/// front-runner entirely.
///
/// | component | f=14% | f=20% | closed by `k`? |
/// |---|---|---|---|
/// | (A) the `k` overshoot, honest depth | +0.044 SOL | +0.124 SOL | ✅ yes |
/// | (B) the live `reserve_sol` read | +0.128 SOL | +0.357 SOL | ❌ DECLINED |
///
/// **The accepted quantity, stated:** a permanent, permissionless, per-trigger leak of
/// **0.107-0.207% of Carnage's spend** = **+0.13 / +0.36 SOL** at today's mainnet CRIME
/// depth (R = 1648.91). ⚠️ The **ratio** bound is permanent; the **absolute** number is
/// NOT -- it scales **LINEARLY WITH POOL DEPTH**, so at 10x CRIME depth the same mechanism
/// is worth **+1.28 / +3.57 SOL**. (An earlier draft of this comment said the residual
/// *"grows ... as a pool thins"*. That INVERTED the depth term: a thinner pool does not
/// grow the leak, it grows the leak's REACHABILITY, because the vault can more easily fund
/// a full-cap spend.)
///
/// ⚠️ It is **NOT** bounded by priority fees. An earlier draft called the leak
/// *"unprofitable ... under a competitive bundle's priority fees"* -- that is **unsound**:
/// the attacker triggers Carnage themselves and composes buy -> carnage -> sell as three
/// top-level instructions in **one transaction**, so there is no auction to win and no
/// mempool race to price them out of.
///
/// ## 🎯 THE MONITORABLE TRIPWIRE (`SOS-H067` residual (d)) -- this is what is watched
///
/// Reachability is gated by how much of a full-cap spend the vault can actually fund:
///
/// ```text
///     carnage sol_vault / ((f/k) * reserve_sol)  ->  1
/// ```
///
/// i.e. **the carnage SOL vault approaching ~6.5-9.3% of the target pool's SOL depth**
/// (`f_min/k` .. `f_max/k`). Observed 2026-08: **1.54 SOL against a ~115 SOL cap -- a 75x
/// gap.** The acceptance is safe while that ratio stays near 1/75 and must be re-derived
/// if it approaches 1. Depth, not the SOL figure, is the alarm.
///
/// Derivation + simulation table: `.docs/163.1/NS-1-carnage-cap-derivation.md` §4; the
/// live-read pool-inflation mechanism (H067): §5.4.
pub const CARNAGE_CAP_K_NUM: u64 = 25;

/// Denominator of the NS-1 zero-EV divisor `k = CARNAGE_CAP_K_DEN / CARNAGE_CAP_K_NUM`.
///
/// `54 / 25 = 2.16`. See [`CARNAGE_CAP_K_NUM`] for the whole derivation -- that doc block
/// is the single canonical explanation and this one deliberately does not repeat it.
pub const CARNAGE_CAP_K_DEN: u64 = 54;

/// 🚨 **BUILD-TIME PROOF THAT THE CAP IS STRICTLY UNDER THE SANDWICH BREAK-EVEN.**
///
/// This is mlbob's Q7 ruling (Disposition Sitting #1, 2026-08-13) turned into a compile
/// error. The old reopen clause on `SOS-H072` / `SOS-H077` was the vague *"if NS-1 is ever
/// weakened, reopen"*; Q7 **rewrote it as a checkable numeric condition** --
/// *"the cap sits strictly below `R(sqrt(1+f) - 1)` across the full tax range"* -- with the
/// recorded reasoning that **NS-1 was never weakened, it was never as strong as
/// described.** A condition nobody can evaluate is how that went unnoticed for months, so
/// it is evaluated here, by rustc, on every build.
///
/// ## The condition, with the square root eliminated
///
/// Want: `cap < D*` for every reachable `f`, i.e. `(NUM/DEN)*f < sqrt(1+f) - 1`.
/// Both sides are positive, so add 1 and square (monotone on positives):
///
/// ```text
///     (1 + f*NUM/DEN)^2 < 1 + f
///     2*f*NUM/DEN + f^2*NUM^2/DEN^2 < f          | -1, and both sides share a factor f > 0
///     2*NUM/DEN + f*NUM^2/DEN^2 < 1              | / f
///     20_000*NUM*DEN + f_bps*NUM^2 < 10_000*DEN^2   | * 10_000*DEN^2, with f = f_bps/10_000
/// ```
///
/// Exact in integers -- no sqrt, no floats, no rounding. The left side is INCREASING in
/// `f_bps`, so checking it at [`CARNAGE_FRICTION_BPS_MAX`] proves it for the whole range.
///
/// ## What it actually rejects
///
/// At `f_bps = 2000`: shipped `25/54` gives `28_250_000 < 29_160_000` ✅. The old `k = 2`
/// (as `1/2` or `25/50`) gives `42_000 < 40_000` ✗ and `26_250_000 < 25_000_000` ✗ -- **it
/// fails at BOTH ends of the tax range**, which is exactly the defect DEC-01 exists to
/// close. `k = 2.0954` fails at f=2000 by 98,840, pinning the 2.0954 floor to the unit.
const _: () = assert!(
    20_000 * CARNAGE_CAP_K_NUM * CARNAGE_CAP_K_DEN
        + (CARNAGE_FRICTION_BPS_MAX as u64) * CARNAGE_CAP_K_NUM * CARNAGE_CAP_K_NUM
        < 10_000 * CARNAGE_CAP_K_DEN * CARNAGE_CAP_K_DEN,
    "NS-1 ZERO-EV VIOLATED: the carnage buy cap is NOT strictly below the exact sandwich \
     break-even R*(sqrt(1+f)-1) at the maximum reachable friction. This is mlbob's Q7 \
     condition (2026-08-13) and it is the whole point of the constant -- a cap at or above \
     break-even makes a self-sandwich around Carnage profitable by construction. Raise \
     CARNAGE_CAP_K_DEN / lower CARNAGE_CAP_K_NUM until k = DEN/NUM >= 2.0954, and re-derive \
     SOS-H067 before shipping."
);

/// Sanity pin: `k` must stay a DIVISOR (k > 1), or the cap would exceed the pool's own
/// linear-impact size and NS-1 would be cushioning nothing.
const _: () = assert!(
    CARNAGE_CAP_K_DEN > CARNAGE_CAP_K_NUM,
    "CARNAGE_CAP_K_DEN / CARNAGE_CAP_K_NUM must be > 1 -- k is a divisor, not a multiplier"
);

/// Minimum possible round-trip friction for any token, in basis points.
///
/// `derive_taxes` ALWAYS pairs one LOW rate with one HIGH rate per token (a token is
/// cheap to buy and expensive to sell, or the reverse -- never expensive on both
/// sides), so `buy + sell` is bounded by `min(LOW) + min(HIGH) = 100 + 1100 = 1200`,
/// plus `2 * AMM_FEE_BPS`.
pub const CARNAGE_FRICTION_BPS_MIN: u16 = 1_400;

/// Maximum possible round-trip friction for any token, in basis points.
///
/// `max(LOW) + max(HIGH) = 400 + 1400 = 1800`, plus `2 * AMM_FEE_BPS`.
///
/// ⚠️ NOT 3000. That figure assumes HIGH + HIGH (1400 + 1400 + 200), which the
/// asymmetric flip makes STRUCTURALLY IMPOSSIBLE -- see `tax_derivation.rs` where the
/// `match cheap_side` arms hand every token exactly one low and one high.
/// `friction_range_excludes_the_impossible_high_high_pair` pins this.
pub const CARNAGE_FRICTION_BPS_MAX: u16 = 2_000;

// ---------------------------------------------------------------------------
// Carnage PDA Seeds
// ---------------------------------------------------------------------------

/// Seed for CarnageFundState PDA.
/// Single global account: seeds = ["carnage_fund"]
pub const CARNAGE_FUND_SEED: &[u8] = b"carnage_fund";

/// Seed for Carnage SOL vault PDA.
/// SystemAccount holding native SOL: seeds = ["carnage_sol_vault"]
pub const CARNAGE_SOL_VAULT_SEED: &[u8] = b"carnage_sol_vault";

/// Seed for Carnage CRIME token vault PDA.
/// Token-2022 account: seeds = ["carnage_crime_vault"]
pub const CARNAGE_CRIME_VAULT_SEED: &[u8] = b"carnage_crime_vault";

/// Seed for Carnage FRAUD token vault PDA.
/// Token-2022 account: seeds = ["carnage_fraud_vault"]
pub const CARNAGE_FRAUD_VAULT_SEED: &[u8] = b"carnage_fraud_vault";

/// Anchor discriminator for Tax Program's swap_exempt instruction.
/// Computed: sha256("global:swap_exempt")[0..8]
/// Used when building CPI instruction data for Carnage swap operations.
/// Promoted from instruction files in Phase 82 for single source of truth.
pub const SWAP_EXEMPT_DISCRIMINATOR: [u8; 8] = [0xf4, 0x5f, 0x5a, 0x24, 0x99, 0xa0, 0x37, 0x0c];

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify discriminator matches sha256("global:update_cumulative")[0..8].
    /// This test documents how the discriminator was derived and ensures
    /// it remains correct if the instruction name changes.
    #[test]
    fn test_update_cumulative_discriminator() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"global:update_cumulative");
        let result = hasher.finalize();
        let expected: [u8; 8] = result[0..8].try_into().unwrap();
        assert_eq!(
            UPDATE_CUMULATIVE_DISCRIMINATOR, expected,
            "Discriminator mismatch: expected {:02x?}, got {:02x?}",
            expected, UPDATE_CUMULATIVE_DISCRIMINATOR
        );
    }

    /// Verify staking authority seed is correct string.
    /// CRITICAL: Must match stub-staking's STAKING_AUTHORITY_SEED.
    #[test]
    fn test_staking_authority_seed() {
        assert_eq!(
            STAKING_AUTHORITY_SEED, b"staking_authority",
            "Seed must match stub-staking's expected seed"
        );
    }

    /// Verify seed matches stub-staking expectation.
    /// Cross-reference: programs/stub-staking/src/lib.rs
    #[test]
    fn test_staking_authority_seed_length() {
        // 17 bytes: "staking_authority"
        assert_eq!(STAKING_AUTHORITY_SEED.len(), 17);
    }

    #[test]
    fn test_tax_program_id() {
        let id = tax_program_id();
        #[cfg(not(feature = "devnet"))]
        assert_eq!(
            id.to_string(),
            "43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj"
        );
        #[cfg(feature = "devnet")]
        assert_eq!(
            id.to_string(),
            "5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ"
        );
    }

    #[test]
    fn test_amm_program_id() {
        let id = amm_program_id();
        #[cfg(not(feature = "devnet"))]
        assert_eq!(
            id.to_string(),
            "5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR"
        );
        #[cfg(feature = "devnet")]
        assert_eq!(
            id.to_string(),
            "AG7QjhAoUecWK8iDDtzB4RzQY2HA7RgHbPeZimJSGwU1"
        );
    }

    #[test]
    fn test_staking_program_id() {
        let id = staking_program_id();
        #[cfg(not(feature = "devnet"))]
        assert_eq!(
            id.to_string(),
            "12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH"
        );
        #[cfg(feature = "devnet")]
        assert_eq!(
            id.to_string(),
            "3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC"
        );
    }

    /// Verify swap_exempt discriminator matches sha256("global:swap_exempt")[0..8].
    /// This test documents how the discriminator was derived and ensures
    /// it remains correct if the instruction name changes.
    #[test]
    fn test_swap_exempt_discriminator() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"global:swap_exempt");
        let result = hasher.finalize();
        let expected: [u8; 8] = result[0..8].try_into().unwrap();
        assert_eq!(
            SWAP_EXEMPT_DISCRIMINATOR, expected,
            "Discriminator mismatch: expected {:02x?}, got {:02x?}",
            expected, SWAP_EXEMPT_DISCRIMINATOR
        );
    }

    #[test]
    fn test_orao_vrf_program_id_is_not_system() {
        // Verify it's not a placeholder
        assert_ne!(
            ORAO_VRF_PROGRAM_ID.to_string(),
            "11111111111111111111111111111111",
            "ORAO_VRF_PROGRAM_ID should not be System Program"
        );
    }

    /// The compile-time assert cannot cover this one: the derivation helper
    /// calls `Pubkey::find_program_address`, which is not a `const fn`. This
    /// test is the substitute gate -- if the literal ever drifts from the
    /// derivation, this fails.
    #[test]
    fn orao_network_state_matches_the_derived_pda() {
        assert_eq!(
            ORAO_NETWORK_STATE,
            orao_solana_vrf::network_state_account_address(&ORAO_VRF_PROGRAM_ID),
            "ORAO_NETWORK_STATE must equal network_state_account_address(ORAO_VRF_PROGRAM_ID)"
        );
    }

    // ---- Phase 47: Carnage Hardening Constants ----

    /// Verify atomic slippage floor is 85% (8500 bps).
    /// The 15% tolerance covers normal same-TX deviations; MEV defense
    /// is primarily atomicity + VRF unpredictability.
    #[test]
    fn test_carnage_slippage_bps_atomic() {
        assert_eq!(CARNAGE_SLIPPAGE_BPS_ATOMIC, 8500);
        // 85% floor: expected * 8500 / 10000
    }

    /// Verify fallback slippage floor is 75% (7500 bps).
    /// More lenient than atomic -- prioritize execution over optimal price
    /// in recovery mode (after lock window expires).
    #[test]
    fn test_carnage_slippage_bps_fallback() {
        assert_eq!(CARNAGE_SLIPPAGE_BPS_FALLBACK, 7500);
        // 75% floor: expected * 7500 / 10000
    }

    /// Verify lock window is 50 slots (~20 seconds at 400ms/slot).
    /// During this window, only the atomic Carnage path can execute.
    /// Must be less than CARNAGE_DEADLINE_SLOTS to leave room for fallback.
    #[test]
    fn test_carnage_lock_slots() {
        assert_eq!(CARNAGE_LOCK_SLOTS, 50);
        // Must be less than CARNAGE_DEADLINE_SLOTS
        assert!(CARNAGE_LOCK_SLOTS < CARNAGE_DEADLINE_SLOTS);
    }

    /// Verify Carnage deadline was increased to 300 slots in Phase 47.
    /// Total window: 0-50 = atomic-only lock, 50-300 = fallback allowed, >300 = expired.
    #[test]
    fn test_carnage_deadline_slots_updated() {
        // Phase 47 increased from 100 to 300 slots (~2 minutes)
        assert_eq!(CARNAGE_DEADLINE_SLOTS, 300);
    }

    /// Verify the lock window is well within the deadline, leaving adequate
    /// room for fallback execution. The fallback window should be at least
    /// 200 slots (~80 seconds) to allow multiple retry attempts.
    #[test]
    fn test_lock_window_within_deadline() {
        // Lock window must expire before fallback deadline
        // Lock: 50 slots. Deadline: 300 slots.
        // Fallback window: slots 50-300.
        assert!(CARNAGE_LOCK_SLOTS < CARNAGE_DEADLINE_SLOTS);
        let fallback_window = CARNAGE_DEADLINE_SLOTS - CARNAGE_LOCK_SLOTS;
        assert!(
            fallback_window >= 200,
            "Fallback window should be >= 200 slots"
        );
    }

    #[test]
    fn test_slots_per_epoch_value() {
        // This is only an init-default/devnet reference after the incremental
        // clock migration. Runtime length is live-read from
        // ArbConfig::epoch_length_slots; launch 4_500 is a ceremony argument.
        assert!(
            (MIN_EPOCH_LENGTH_SLOTS..=MAX_EPOCH_LENGTH_SLOTS).contains(&SLOTS_PER_EPOCH),
            "SLOTS_PER_EPOCH reference must stay inside ArbConfig bounds, got {}",
            SLOTS_PER_EPOCH
        );
    }

    #[test]
    fn test_trigger_bounty_lamports() {
        // 0.001 SOL = 1,000,000 lamports
        assert_eq!(TRIGGER_BOUNTY_LAMPORTS, 1_000_000);
    }

    // ---- Phase 83: EpochState Layout Validation (VRF-09) ----

    /// Validate EpochState Borsh-serialized byte offsets match TypeScript EPOCH_STATE_OFFSETS.
    ///
    /// This test serializes a known EpochState value with recognizable byte patterns,
    /// then reads specific byte positions to verify the layout matches documented offsets.
    /// If a field is added/removed/resized, this test WILL fail, catching layout drift.
    ///
    /// Offsets documented here are DATA offsets (without 8-byte discriminator).
    /// To get on-chain byte offset, add 8.
    ///
    /// Cross-reference: tests/integration/helpers/mock-vrf.ts EPOCH_STATE_OFFSETS
    #[test]
    fn test_epoch_state_serialized_offsets() {
        use crate::state::EpochState;
        use anchor_lang::AnchorSerialize;

        let state = EpochState {
            genesis_slot: 0x0807060504030201,      // LE: 01 02 03 04 05 06 07 08
            current_epoch: 0x0C0B0A09,             // LE: 09 0A 0B 0C
            epoch_start_slot: 0x100F0E0D_14131211, // arbitrary
            cheap_side: 0xAA,
            low_tax_bps: 0xBBCC,
            high_tax_bps: 0xDDEE,
            crime_buy_tax_bps: 0x1122,
            crime_sell_tax_bps: 0x3344,
            fraud_buy_tax_bps: 0x5566,
            fraud_sell_tax_bps: 0x7788,
            vrf_request_slot: 0xAAAAAAAABBBBBBBB,
            vrf_pending: true,
            taxes_confirmed: false,
            pending_randomness_account: Pubkey::new_from_array([0xFF; 32]),
            carnage_pending: true,
            carnage_target: 0x42,
            carnage_action: 0x43,
            carnage_deadline_slot: 0xDEADBEEFCAFEBABE,
            carnage_lock_slot: 0x1234567890ABCDEF,
            last_carnage_epoch: 0xFEEDFACE,
            pause_end_slot: 0x1122334455667788,
            trading_paused: true,
            safety_version: 0,
            vrf_transition_start_slot: 0,
            pending_seed_slot: 0,
            vrf_attempts: 0,
            carnage_generation: 0,
            last_degraded_epoch: 0,
            last_vrf_fallback_reason: 0,
            reserved: [0; 28],
            initialized: true,
            bump: 0xFD,
        };

        let mut buf = Vec::new();
        state.serialize(&mut buf).unwrap();

        // Verify total serialized size (without discriminator)
        assert_eq!(
            buf.len(),
            EpochState::DATA_LEN,
            "Serialized size must match DATA_LEN (164 bytes)"
        );

        // --- Verify field offsets by checking recognizable byte patterns ---
        // All offsets below are DATA offsets (on-chain = offset + 8)

        // genesis_slot at data offset 0 (on-chain 8): 0x0807060504030201 LE
        assert_eq!(buf[0], 0x01, "genesis_slot[0] at data offset 0");
        assert_eq!(buf[7], 0x08, "genesis_slot[7] at data offset 7");

        // current_epoch at data offset 8 (on-chain 16): 0x0C0B0A09 LE
        assert_eq!(buf[8], 0x09, "current_epoch[0] at data offset 8");
        assert_eq!(buf[11], 0x0C, "current_epoch[3] at data offset 11");

        // epoch_start_slot at data offset 12 (on-chain 20)
        // cheap_side at data offset 20 (on-chain 28)
        assert_eq!(buf[20], 0xAA, "cheap_side at data offset 20");

        // low_tax_bps at data offset 21 (on-chain 29): 0xBBCC LE
        assert_eq!(buf[21], 0xCC, "low_tax_bps[0] at data offset 21");
        assert_eq!(buf[22], 0xBB, "low_tax_bps[1] at data offset 22");

        // high_tax_bps at data offset 23 (on-chain 31): 0xDDEE LE
        assert_eq!(buf[23], 0xEE, "high_tax_bps[0] at data offset 23");

        // crime_buy_tax_bps at data offset 25 (on-chain 33)
        assert_eq!(buf[25], 0x22, "crime_buy_tax_bps[0] at data offset 25");

        // vrf_request_slot at data offset 33 (on-chain 41)
        assert_eq!(buf[33], 0xBB, "vrf_request_slot[0] at data offset 33");

        // vrf_pending at data offset 41 (on-chain 49): true = 1
        assert_eq!(buf[41], 1, "vrf_pending at data offset 41");

        // taxes_confirmed at data offset 42 (on-chain 50): false = 0
        assert_eq!(buf[42], 0, "taxes_confirmed at data offset 42");

        // pending_randomness_account at data offset 43 (on-chain 51): all 0xFF
        assert_eq!(
            buf[43], 0xFF,
            "pending_randomness_account[0] at data offset 43"
        );
        assert_eq!(
            buf[74], 0xFF,
            "pending_randomness_account[31] at data offset 74"
        );

        // carnage_pending at data offset 75 (on-chain 83): true = 1
        assert_eq!(buf[75], 1, "carnage_pending at data offset 75");

        // carnage_target at data offset 76 (on-chain 84)
        assert_eq!(buf[76], 0x42, "carnage_target at data offset 76");

        // carnage_action at data offset 77 (on-chain 85)
        assert_eq!(buf[77], 0x43, "carnage_action at data offset 77");

        // carnage_deadline_slot at data offset 78 (on-chain 86): 0xDEADBEEFCAFEBABE LE
        assert_eq!(buf[78], 0xBE, "carnage_deadline_slot[0] at data offset 78");

        // carnage_lock_slot at data offset 86 (on-chain 94): 0x1234567890ABCDEF LE
        assert_eq!(buf[86], 0xEF, "carnage_lock_slot[0] at data offset 86");
        assert_eq!(buf[93], 0x12, "carnage_lock_slot[7] at data offset 93");

        // last_carnage_epoch at data offset 94 (on-chain 102): 0xFEEDFACE LE
        assert_eq!(buf[94], 0xCE, "last_carnage_epoch[0] at data offset 94");
        assert_eq!(buf[97], 0xFE, "last_carnage_epoch[3] at data offset 97");

        assert_eq!(buf[98], 0x88, "pause_end_slot[0] at data offset 98");
        assert_eq!(buf[105], 0x11, "pause_end_slot[7] at data offset 105");
        assert_eq!(buf[106], 1, "trading_paused at data offset 106");

        // Safety carve + remaining reserved bytes occupy the old 55-byte
        // reserved window at data offset 107 (on-chain 115).
        for i in 0..55 {
            assert_eq!(
                buf[107 + i],
                0,
                "reserved[{}] at data offset {}",
                i,
                107 + i
            );
        }

        // initialized at data offset 162 (on-chain 170): true = 1
        assert_eq!(buf[162], 1, "initialized at data offset 162");

        // bump at data offset 163 (on-chain 171)
        assert_eq!(buf[163], 0xFD, "bump at data offset 163");
    }

    // ---- Phase 83: Anti-Reroll Documentation (VRF-06) ----

    /// Anti-reroll protection test documentation (VRF-06):
    ///
    /// The `consume_randomness` instruction has a constraint:
    ///   `constraint = randomness_account.key() == epoch_state.pending_randomness_account`
    ///
    /// Attempting to consume with a different randomness account triggers:
    ///   AnchorError { error_code_number: 2012, error_msg: "A raw constraint was violated" }
    ///   (ConstraintRaw / 0x07DC)
    ///
    /// This is validated in the LiteSVM integration tests at:
    ///   tests/integration/cpi-chains.test.ts
    ///
    /// The specific assertion should be:
    ///   expect(error.error.errorCode.number).toBe(2012);
    #[test]
    fn test_anti_reroll_error_code_documented() {
        // Anchor ConstraintRaw error code.
        // When consume_randomness receives a randomness account that doesn't match
        // pending_randomness_account, Anchor rejects with error 2012.
        const CONSTRAINT_RAW_ERROR_CODE: u32 = 2012;
        assert_eq!(
            CONSTRAINT_RAW_ERROR_CODE, 2012,
            "Anti-reroll uses Anchor ConstraintRaw"
        );

        // Hex representation for cross-referencing with on-chain errors
        assert_eq!(CONSTRAINT_RAW_ERROR_CODE, 0x07DC, "ConstraintRaw hex value");
    }
}
