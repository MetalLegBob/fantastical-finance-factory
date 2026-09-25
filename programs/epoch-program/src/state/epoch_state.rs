//! EpochState account structure.
//!
//! Global singleton that governs tax regime transitions, VRF integration,
//! and Carnage Fund execution.
//!
//! Source: Epoch_State_Machine_Spec.md Section 4.1

use anchor_lang::prelude::*;

/// Global epoch state account.
///
/// Single PDA: seeds = ["epoch_state"]
///
/// This account is the coordination hub for all protocol dynamics:
/// - Tax rates (read by Tax Program during swaps)
/// - VRF state (commit-reveal randomness lifecycle)
/// - Carnage state (pending execution tracking)
///
/// **Size calculation:**
/// - Discriminator: 8 bytes
/// - genesis_slot: 8 bytes
/// - current_epoch: 4 bytes
/// - epoch_start_slot: 8 bytes
/// - cheap_side: 1 byte
/// - low_tax_bps: 2 bytes
/// - high_tax_bps: 2 bytes
/// - crime_buy_tax_bps: 2 bytes
/// - crime_sell_tax_bps: 2 bytes
/// - fraud_buy_tax_bps: 2 bytes
/// - fraud_sell_tax_bps: 2 bytes
/// - vrf_request_slot: 8 bytes
/// - vrf_pending: 1 byte
/// - taxes_confirmed: 1 byte
/// - pending_randomness_account: 32 bytes
/// - carnage_pending: 1 byte
/// - carnage_target: 1 byte
/// - carnage_action: 1 byte
/// - carnage_deadline_slot: 8 bytes
/// - carnage_lock_slot: 8 bytes
/// - last_carnage_epoch: 4 bytes
/// - pause_end_slot: 8 bytes
/// - trading_paused: 1 byte
/// - safety_version: 1 byte
/// - vrf_transition_start_slot: 8 bytes
/// - pending_seed_slot: 8 bytes
/// - vrf_attempts: 1 byte
/// - carnage_generation: 4 bytes
/// - last_degraded_epoch: 4 bytes
/// - last_vrf_fallback_reason: 1 byte
/// - reserved: 28 bytes (future schema evolution padding)
/// - initialized: 1 byte
/// - bump: 1 byte
/// Total: 8 + 164 = 172 bytes
///
/// Source: Epoch_State_Machine_Spec.md Section 4.1, Phase 47 CONTEXT.md
#[account]
#[repr(C)]
pub struct EpochState {
    // =========================================================================
    // Timing (20 bytes)
    // =========================================================================
    /// Slot when protocol was initialized (genesis).
    /// Retained as immutable display/reference data after the incremental-clock cutover.
    pub genesis_slot: u64,

    /// Current epoch number (0-indexed).
    /// Increments each time trigger_epoch_transition succeeds.
    pub current_epoch: u32,

    /// Slot when the current epoch started.
    /// Re-anchored to clock.slot after every successful transition.
    pub epoch_start_slot: u64,

    // =========================================================================
    // Tax Configuration - Active (7 bytes)
    // =========================================================================
    /// Current cheap side: 0 = CRIME, 1 = FRAUD.
    /// Cheap side gets low tax on buy, high tax on sell.
    pub cheap_side: u8,

    /// Low tax rate in basis points (100-400, i.e., 1-4%).
    /// Applied to cheap side buys and expensive side sells.
    pub low_tax_bps: u16,

    /// High tax rate in basis points (1100-1400, i.e., 11-14%).
    /// Applied to cheap side sells and expensive side buys.
    pub high_tax_bps: u16,

    // =========================================================================
    // Derived Tax Rates - Cached (8 bytes)
    // =========================================================================
    /// CRIME buy tax rate in basis points.
    /// If CRIME cheap: low_tax_bps. If FRAUD cheap: high_tax_bps.
    pub crime_buy_tax_bps: u16,

    /// CRIME sell tax rate in basis points.
    /// If CRIME cheap: high_tax_bps. If FRAUD cheap: low_tax_bps.
    pub crime_sell_tax_bps: u16,

    /// FRAUD buy tax rate in basis points.
    /// If FRAUD cheap: low_tax_bps. If CRIME cheap: high_tax_bps.
    pub fraud_buy_tax_bps: u16,

    /// FRAUD sell tax rate in basis points.
    /// If FRAUD cheap: high_tax_bps. If CRIME cheap: low_tax_bps.
    pub fraud_sell_tax_bps: u16,

    // =========================================================================
    // VRF State (42 bytes)
    // =========================================================================
    /// Slot when VRF randomness was committed (0 = none pending).
    /// Used for timeout detection: if current_slot > vrf_request_slot + VRF_TIMEOUT_SLOTS, retry allowed.
    pub vrf_request_slot: u64,

    /// Whether a VRF request is pending (waiting for consume_randomness).
    pub vrf_pending: bool,

    /// Whether taxes have been confirmed for the current epoch.
    /// False between trigger_epoch_transition and consume_randomness.
    pub taxes_confirmed: bool,

    /// Pubkey of the ORAO randomness REQUEST PDA bound when the request was
    /// issued, i.e. randomness_account_address(ORAO, epoch_vrf_seed(..)).
    /// Anti-reroll protection: consume_randomness must use this exact account.
    pub pending_randomness_account: Pubkey,

    // =========================================================================
    // Carnage State (23 bytes)
    // =========================================================================
    /// Whether Carnage execution is pending (atomic failed, fallback active).
    pub carnage_pending: bool,

    /// Target token for Carnage buy: 0 = CRIME, 1 = FRAUD.
    /// Only valid when carnage_pending = true.
    pub carnage_target: u8,

    /// Carnage action type: 0 = None, 1 = Burn, 2 = Sell.
    /// Only valid when carnage_pending = true.
    pub carnage_action: u8,

    /// Slot deadline for fallback Carnage execution.
    /// If current_slot > carnage_deadline_slot, Carnage expires.
    pub carnage_deadline_slot: u64,

    /// Slot until which only the atomic Carnage path can execute.
    /// After lock expires, fallback execute_carnage becomes callable.
    /// Set to current_slot + CARNAGE_LOCK_SLOTS when Carnage triggers.
    pub carnage_lock_slot: u64,

    /// Last epoch when Carnage was triggered.
    /// Used to track Carnage frequency.
    pub last_carnage_epoch: u32,

    // =========================================================================
    // Reserved Padding (64 bytes total) -- DEF-03
    //
    // Future schema evolution: add new fields by consuming reserved bytes.
    // This avoids account migration on schema changes.
    // =========================================================================
    /// Absolute slot at which the post-transition trading pause ends (exclusive).
    /// Written at every epoch transition as clock.slot + arb_config.pause_slots (spec §14.2 correction).
    /// Pre-upgrade bytes here are zero => 0 => "no pause" until the first post-upgrade transition.
    pub pause_end_slot: u64,

    /// Manual protocol trading-pause flag (spec §14.3 ruling 3, §14.6).
    /// Set by pause_authority OR pause_tripwire; cleared by pause_authority ONLY.
    /// Pre-upgrade bytes are zero => false. Flag only — who/when live in events.
    pub trading_paused: bool,

    /// Reserved-byte schema version. Zero is the pre-remediation layout and is
    /// deliberately rejected until `activate_epoch_safety` runs while the
    /// singleton has no pending VRF or Carnage work.
    pub safety_version: u8,

    /// Immutable start slot for the current VRF generation. Unlike
    /// `vrf_request_slot`, this is never changed by a retry.
    pub vrf_transition_start_slot: u64,

    /// The slot at which the ORAO randomness request was issued (A-05).
    ///
    /// SEMANTIC CHANGE, NOT A LAYOUT CHANGE. This field previously held the
    /// oracle's own commitment seed-slot snapshot, and consume asserted
    /// equality against it. Under ORAO the equivalent bind is re-deriving the
    /// request PDA from the seed, so this now records the request slot for
    /// diagnostics and the timeout arithmetic. The name, type, size and
    /// offset are all unchanged: EpochState's layout is frozen (D-06) and the
    /// Tax program, the keeper decoder and the app decoder read it at a fixed
    /// offset with no window to re-point them.
    pub pending_seed_slot: u64,

    /// Number of VRF commitments made for this transition, including the
    /// initial trigger commitment.
    pub vrf_attempts: u8,

    /// Epoch/generation to which pending Carnage belongs. Zero when no Carnage
    /// is pending; callers must pass this generation explicitly.
    pub carnage_generation: u32,

    /// Most recent epoch resolved via deterministic degraded fallback. The
    /// companion reason is zero when no degraded fallback has ever occurred.
    pub last_degraded_epoch: u32,

    /// Zero = none, 1 = final attempt timed out, 2 = absolute age reached.
    pub last_vrf_fallback_reason: u8,

    /// Remaining reserved padding (64 total, with 36 bytes carved so far).
    pub reserved: [u8; 28],

    // =========================================================================
    // Protocol (2 bytes)
    // =========================================================================
    /// Whether the epoch state has been initialized.
    /// Set to true in initialize_epoch_state, prevents re-initialization.
    pub initialized: bool,

    /// PDA bump seed.
    pub bump: u8,
}

impl EpochState {
    /// Total account size including 8-byte discriminator.
    /// 8 (discriminator) + 164 (data) = 172 bytes.
    ///
    /// Source: Phase 47 added carnage_lock_slot (u64, +8 bytes).
    /// Phase 80 added reserved padding (+64 bytes).
    pub const LEN: usize = 8 + Self::DATA_LEN;

    /// Calculate data size without discriminator (for verification).
    /// Should equal 164 bytes.
    /// Layout: genesis_slot(8) + current_epoch(4) + epoch_start_slot(8)
    ///       + cheap_side(1) + low_tax_bps(2) + high_tax_bps(2)
    ///       + crime_buy_tax_bps(2) + crime_sell_tax_bps(2) + fraud_buy_tax_bps(2) + fraud_sell_tax_bps(2)
    ///       + vrf_request_slot(8) + vrf_pending(1) + taxes_confirmed(1) + pending_randomness_account(32)
    ///       + carnage_pending(1) + carnage_target(1) + carnage_action(1) + carnage_deadline_slot(8) + carnage_lock_slot(8) + last_carnage_epoch(4)
    ///       + pause_end_slot(8) + trading_paused(1)
    ///       + safety_version(1) + vrf_transition_start_slot(8) + pending_seed_slot(8)
    ///       + vrf_attempts(1) + carnage_generation(4) + last_degraded_epoch(4)
    ///       + last_vrf_fallback_reason(1) + reserved(28)
    ///       + initialized(1) + bump(1)
    pub const DATA_LEN: usize = 8
        + 4
        + 8
        + 1
        + 2
        + 2
        + 2
        + 2
        + 2
        + 2
        + 8
        + 1
        + 1
        + 32
        + 1
        + 1
        + 1
        + 8
        + 8
        + 4
        + 8
        + 1
        + 1
        + 8
        + 8
        + 1
        + 4
        + 4
        + 1
        + 28
        + 1
        + 1;
}

// DEF-08: Compile-time assertion that DATA_LEN matches expected value.
// If this fails, the struct layout has drifted from the documented size.
const _: () = assert!(EpochState::DATA_LEN == 164);

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::{AnchorDeserialize, AnchorSerialize};

    /// Proves a pre-carve account's all-zero reserved bytes decode as both
    /// pause mechanisms disabled, so the upgrade needs no migration instruction.
    #[test]
    fn test_pre_carve_bytes_decode_as_no_pause() {
        let original = EpochState {
            genesis_slot: 101,
            current_epoch: 202,
            epoch_start_slot: 303,
            cheap_side: 1,
            low_tax_bps: 400,
            high_tax_bps: 1_100,
            crime_buy_tax_bps: 1_100,
            crime_sell_tax_bps: 400,
            fraud_buy_tax_bps: 400,
            fraud_sell_tax_bps: 1_100,
            vrf_request_slot: 404,
            vrf_pending: true,
            taxes_confirmed: false,
            pending_randomness_account: Pubkey::new_unique(),
            carnage_pending: true,
            carnage_target: 1,
            carnage_action: 2,
            carnage_deadline_slot: 505,
            carnage_lock_slot: 606,
            last_carnage_epoch: 201,
            pause_end_slot: 0,
            trading_paused: false,
            safety_version: 0,
            vrf_transition_start_slot: 0,
            pending_seed_slot: 0,
            vrf_attempts: 0,
            carnage_generation: 0,
            last_degraded_epoch: 0,
            last_vrf_fallback_reason: 0,
            reserved: [0u8; 28],
            initialized: true,
            bump: 254,
        };

        let mut pre_carve_bytes = Vec::new();
        original
            .serialize(&mut pre_carve_bytes)
            .expect("serialize pre-carve-compatible state");

        assert_eq!(pre_carve_bytes.len(), EpochState::DATA_LEN);
        assert!(
            pre_carve_bytes[98..162].iter().all(|byte| *byte == 0),
            "the old 64-byte reserved region must remain entirely zero"
        );

        let recovered = EpochState::deserialize(&mut pre_carve_bytes.as_slice())
            .expect("deserialize pre-carve bytes with carved layout");

        assert_eq!(recovered.genesis_slot, original.genesis_slot);
        assert_eq!(recovered.current_epoch, original.current_epoch);
        assert_eq!(recovered.epoch_start_slot, original.epoch_start_slot);
        assert_eq!(recovered.cheap_side, original.cheap_side);
        assert_eq!(recovered.low_tax_bps, original.low_tax_bps);
        assert_eq!(recovered.high_tax_bps, original.high_tax_bps);
        assert_eq!(recovered.crime_buy_tax_bps, original.crime_buy_tax_bps);
        assert_eq!(recovered.crime_sell_tax_bps, original.crime_sell_tax_bps);
        assert_eq!(recovered.fraud_buy_tax_bps, original.fraud_buy_tax_bps);
        assert_eq!(recovered.fraud_sell_tax_bps, original.fraud_sell_tax_bps);
        assert_eq!(recovered.vrf_request_slot, original.vrf_request_slot);
        assert_eq!(recovered.vrf_pending, original.vrf_pending);
        assert_eq!(recovered.taxes_confirmed, original.taxes_confirmed);
        assert_eq!(
            recovered.pending_randomness_account,
            original.pending_randomness_account
        );
        assert_eq!(recovered.carnage_pending, original.carnage_pending);
        assert_eq!(recovered.carnage_target, original.carnage_target);
        assert_eq!(recovered.carnage_action, original.carnage_action);
        assert_eq!(
            recovered.carnage_deadline_slot,
            original.carnage_deadline_slot
        );
        assert_eq!(recovered.carnage_lock_slot, original.carnage_lock_slot);
        assert_eq!(recovered.last_carnage_epoch, original.last_carnage_epoch);
        assert_eq!(recovered.pause_end_slot, 0);
        assert!(!recovered.trading_paused);
        assert_eq!(recovered.safety_version, 0);
        assert_eq!(recovered.vrf_transition_start_slot, 0);
        assert_eq!(recovered.pending_seed_slot, 0);
        assert_eq!(recovered.vrf_attempts, 0);
        assert_eq!(recovered.carnage_generation, 0);
        assert_eq!(recovered.last_degraded_epoch, 0);
        assert_eq!(recovered.last_vrf_fallback_reason, 0);
        assert_eq!(recovered.reserved, [0u8; 28]);
        assert_eq!(recovered.initialized, original.initialized);
        assert_eq!(recovered.bump, original.bump);
    }
}
