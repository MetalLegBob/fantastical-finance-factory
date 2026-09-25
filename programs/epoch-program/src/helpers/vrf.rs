//! Pure boundary logic for the bounded VRF lifecycle, plus the domain-separated
//! seed used to address an ORAO randomness request.

use anchor_lang::prelude::Pubkey;

use crate::constants::{MAX_VRF_ATTEMPTS, MAX_VRF_PENDING_SLOTS, VRF_TIMEOUT_SLOTS};

pub const VRF_FALLBACK_REASON_FINAL_ATTEMPT: u8 = 1;
pub const VRF_FALLBACK_REASON_ABSOLUTE_AGE: u8 = 2;

/// Return the single deterministic terminal reason for a pending generation.
/// Absolute age takes precedence when both limits become true in one slot.
pub fn terminal_vrf_reason(
    current_slot: u64,
    transition_start_slot: u64,
    request_slot: u64,
    attempts: u8,
) -> Option<u8> {
    let absolute_age = current_slot.saturating_sub(transition_start_slot);
    if absolute_age >= MAX_VRF_PENDING_SLOTS {
        return Some(VRF_FALLBACK_REASON_ABSOLUTE_AGE);
    }

    let current_attempt_age = current_slot.saturating_sub(request_slot);
    if attempts >= MAX_VRF_ATTEMPTS && current_attempt_age > VRF_TIMEOUT_SLOTS {
        return Some(VRF_FALLBACK_REASON_FINAL_ATTEMPT);
    }

    None
}

/// Domain-separated VRF seed for an ORAO randomness request.
///
/// MUST be derivable OFF-CHAIN before the transaction is sent: ORAO `init`s the
/// request PDA inside our CPI, so the keeper has to list that address in the
/// transaction's account keys. Therefore this function must NEVER depend on
/// `Clock` -- a clock-derived seed makes the trigger unbuildable off-chain, and
/// makes two keepers derive two different PDAs so neither can consume the
/// other's request. See 179.1-RESEARCH.md Pitfall 1.
///
/// Uniqueness argument: `epoch` increments on every trigger and `attempt` on
/// every retry, so `(epoch, attempt)` never repeats within one initialization;
/// `genesis_slot` is immutable and separates a re-initialized devnet EpochState
/// from colliding with old PDAs; `epoch_state` separates clusters and programs.
/// A repeated seed does not replay stale randomness -- ORAO's `init` on an
/// existing PDA fails and the whole trigger reverts -- but it would brick that
/// attempt number, so uniqueness is an invariant to test, not a comment.
///
/// ORAO rejects an all-zero seed. A sha256 output is never zero in practice;
/// "in practice" is not relied upon -- it is proptested.
///
/// Call convention, which MUST be identical across the three call sites or
/// consume can never find the request:
/// - trigger: `epoch = current_epoch + 1`, `attempt = 1`
/// - retry:   `epoch = current_epoch`,     `attempt = vrf_attempts + 1`
/// - consume: `epoch = current_epoch`,     `attempt = vrf_attempts`
pub fn epoch_vrf_seed(
    epoch_state: &Pubkey,
    genesis_slot: u64,
    epoch: u32,
    attempt: u8,
) -> [u8; 32] {
    solana_sha256_hasher::hashv(&[
        b"drf:epoch-vrf:v1",
        epoch_state.as_ref(),
        &genesis_slot.to_le_bytes(),
        &epoch.to_le_bytes(),
        &[attempt],
    ])
    .to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_attempt_gets_its_full_reveal_window() {
        let start = 1_000;
        let request = 1_602;
        assert_eq!(
            terminal_vrf_reason(
                request + VRF_TIMEOUT_SLOTS,
                start,
                request,
                MAX_VRF_ATTEMPTS
            ),
            None
        );
        assert_eq!(
            terminal_vrf_reason(
                request + VRF_TIMEOUT_SLOTS + 1,
                start,
                request,
                MAX_VRF_ATTEMPTS
            ),
            Some(VRF_FALLBACK_REASON_FINAL_ATTEMPT)
        );
    }

    #[test]
    fn absolute_age_boundary_is_inclusive_and_wins_ties() {
        let start = 10;
        assert_eq!(
            terminal_vrf_reason(
                start + MAX_VRF_PENDING_SLOTS,
                start,
                start,
                MAX_VRF_ATTEMPTS
            ),
            Some(VRF_FALLBACK_REASON_ABSOLUTE_AGE)
        );
    }

    #[test]
    fn fewer_attempts_do_not_terminal_before_absolute_age() {
        assert_eq!(terminal_vrf_reason(1_000, 0, 0, 1), None);
    }
}
