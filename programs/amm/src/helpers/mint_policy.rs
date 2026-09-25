//! Shared admission policy for amount-sensitive SPL mint operations.
//!
//! The AMM and Tax program use nominal token amounts for reserve accounting,
//! tax arithmetic, and minimum-output checks.  A transfer-fee mint would make
//! those nominal amounts differ from the amount actually credited.  Such
//! mints are outside the supported asset model, so every operation re-checks
//! both Token-2022 fee schedules before moving value.

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022;
use spl_token_2022::extension::{
    transfer_fee::{TransferFee, TransferFeeConfig},
    BaseStateWithExtensions, ExtensionType, StateWithExtensions,
};
use spl_token_2022::state::Mint;

/// A caller-neutral error so both programs can map the shared parser to their
/// own stable Anchor error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MintPolicyViolation {
    UnsupportedMintOwner,
    InvalidMintData,
    NonZeroTransferFee,
}

/// True only when the complete schedule is amount-preserving.
///
/// Checking both fields is deliberate: a zero-bps schedule with a stale
/// nonzero maximum is rejected too, keeping the supported policy simple and
/// preventing a later bps-only update from silently entering scope.
pub fn transfer_fee_schedule_is_zero(schedule: &TransferFee) -> bool {
    u16::from(schedule.transfer_fee_basis_points) == 0 && u64::from(schedule.maximum_fee) == 0
}

/// Validate a mint for use by nominal-amount accounting.
///
/// Classic SPL Token mints cannot carry extensions and are accepted.  A
/// Token-2022 mint is parsed on every call; when TransferFeeConfig exists,
/// both its current/older and scheduled/newer fee tuples must be zero/zero.
pub fn validate_amount_preserving_mint(
    mint_info: &AccountInfo<'_>,
) -> std::result::Result<(), MintPolicyViolation> {
    if *mint_info.owner == anchor_spl::token::ID {
        return Ok(());
    }
    if *mint_info.owner != anchor_spl::token_2022::ID {
        return Err(MintPolicyViolation::UnsupportedMintOwner);
    }

    let data = mint_info
        .try_borrow_data()
        .map_err(|_| MintPolicyViolation::InvalidMintData)?;
    let mint = StateWithExtensions::<Mint>::unpack(&data)
        .map_err(|_| MintPolicyViolation::InvalidMintData)?;
    let extension_types = mint
        .get_extension_types()
        .map_err(|_| MintPolicyViolation::InvalidMintData)?;

    if extension_types.contains(&ExtensionType::TransferFeeConfig) {
        let config = mint
            .get_extension::<TransferFeeConfig>()
            .map_err(|_| MintPolicyViolation::InvalidMintData)?;
        if !transfer_fee_schedule_is_zero(&config.older_transfer_fee)
            || !transfer_fee_schedule_is_zero(&config.newer_transfer_fee)
        {
            return Err(MintPolicyViolation::NonZeroTransferFee);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(bps: u16, max: u64) -> TransferFee {
        TransferFee {
            epoch: 0u64.into(),
            maximum_fee: max.into(),
            transfer_fee_basis_points: bps.into(),
        }
    }

    #[test]
    fn zero_zero_schedule_is_amount_preserving() {
        assert!(transfer_fee_schedule_is_zero(&schedule(0, 0)));
    }

    #[test]
    fn nonzero_current_or_scheduled_components_are_rejected() {
        assert!(!transfer_fee_schedule_is_zero(&schedule(1, 0)));
        assert!(!transfer_fee_schedule_is_zero(&schedule(0, 1)));
        assert!(!transfer_fee_schedule_is_zero(&schedule(25, 1_000)));
    }
}
