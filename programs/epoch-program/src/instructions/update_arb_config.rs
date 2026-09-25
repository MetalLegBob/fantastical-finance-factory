//! Update ArbConfig instruction.
//!
//! Applies one-shot wallet, pause-key, and timing changes while initiating,
//! overwriting, or cancelling a two-step authority transfer.

use anchor_lang::prelude::*;

use crate::constants::ARB_CONFIG_SEED;
use crate::errors::EpochError;
use crate::events::ArbConfigUpdated;
use crate::state::{
    validate_arb_config_bounds, validate_no_zero_pubkeys, validate_role_overlap, ArbConfig,
};

/// Optional ArbConfig changes applied by the current authority.
///
/// Every supplied wallet, pause key, or timing value takes effect immediately.
/// `new_authority` only changes `pending_authority`; the active authority is
/// changed exclusively by `accept_arb_config_authority`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdateArbConfigArgs {
    pub new_authority: Option<Pubkey>,
    pub new_wallet_a: Option<Pubkey>,
    pub new_wallet_b: Option<Pubkey>,
    pub new_pause_slots: Option<u64>,
    pub new_epoch_length_slots: Option<u64>,
    pub new_pause_authority: Option<Pubkey>,
    pub new_pause_tripwire: Option<Pubkey>,
}

/// Fully resolved post-update values, produced only after validation succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResultingArbConfig {
    authority: Pubkey,
    pending_authority: Pubkey,
    wallet_a: Pubkey,
    wallet_b: Pubkey,
    pause_authority: Pubkey,
    pause_tripwire: Pubkey,
    pause_slots: u64,
    epoch_length_slots: u64,
}

/// Resolve optional changes against unchanged fields and validate the complete
/// resulting state before the handler writes any account field.
fn validate_resulting_config(
    current: &ArbConfig,
    args: &UpdateArbConfigArgs,
) -> Result<ResultingArbConfig> {
    let resulting = ResultingArbConfig {
        authority: current.authority,
        pending_authority: args.new_authority.unwrap_or(current.pending_authority),
        wallet_a: args.new_wallet_a.unwrap_or(current.wallet_a),
        wallet_b: args.new_wallet_b.unwrap_or(current.wallet_b),
        pause_authority: args.new_pause_authority.unwrap_or(current.pause_authority),
        pause_tripwire: args.new_pause_tripwire.unwrap_or(current.pause_tripwire),
        pause_slots: args.new_pause_slots.unwrap_or(current.pause_slots),
        epoch_length_slots: args
            .new_epoch_length_slots
            .unwrap_or(current.epoch_length_slots),
    };

    validate_arb_config_bounds(resulting.pause_slots, resulting.epoch_length_slots)?;
    validate_no_zero_pubkeys(&[
        resulting.authority,
        resulting.wallet_a,
        resulting.wallet_b,
        resulting.pause_authority,
        resulting.pause_tripwire,
    ])?;
    validate_role_overlap(
        &resulting.authority,
        &resulting.wallet_a,
        &resulting.wallet_b,
        &resulting.pause_authority,
        &resulting.pause_tripwire,
    )?;

    // A default new_authority is the explicit pending-transfer cancellation
    // sentinel. It never reaches the active authority field and is not a burn.
    if let Some(pending_authority) = args.new_authority {
        if pending_authority != Pubkey::default() {
            validate_no_zero_pubkeys(&[pending_authority])?;
            validate_role_overlap(
                &pending_authority,
                &resulting.wallet_a,
                &resulting.wallet_b,
                &resulting.pause_authority,
                &resulting.pause_tripwire,
            )?;
        }
    }

    Ok(resulting)
}

/// Update the global ArbConfig singleton.
///
/// The current authority retains every power until a nonzero pending authority
/// accepts in the separate accept instruction. Passing
/// `new_authority = Some(Pubkey::default())` cancels a pending transfer only;
/// it never writes the active authority and therefore cannot burn it.
///
/// Source: Centralized-Arb MVP Spec §3.1 and §14.6 entries 4, 5, and 6.
pub fn handler(ctx: Context<UpdateArbConfig>, args: UpdateArbConfigArgs) -> Result<()> {
    let arb_config = &mut ctx.accounts.arb_config;

    // Resolve and validate first. No account field is written before this
    // function returns successfully.
    let resulting = validate_resulting_config(arb_config, &args)?;
    let slot = Clock::get()?.slot;

    // Active authority is deliberately not assigned here.
    arb_config.pending_authority = resulting.pending_authority;
    arb_config.wallet_a = resulting.wallet_a;
    arb_config.wallet_b = resulting.wallet_b;
    arb_config.pause_authority = resulting.pause_authority;
    arb_config.pause_tripwire = resulting.pause_tripwire;
    arb_config.pause_slots = resulting.pause_slots;
    arb_config.epoch_length_slots = resulting.epoch_length_slots;

    emit!(ArbConfigUpdated {
        authority: arb_config.authority,
        pending_authority: arb_config.pending_authority,
        wallet_a: arb_config.wallet_a,
        wallet_b: arb_config.wallet_b,
        pause_authority: arb_config.pause_authority,
        pause_tripwire: arb_config.pause_tripwire,
        pause_slots: arb_config.pause_slots,
        epoch_length_slots: arb_config.epoch_length_slots,
        slot,
    });

    Ok(())
}

/// Accounts for `update_arb_config`.
#[derive(Accounts)]
pub struct UpdateArbConfig<'info> {
    /// Current ArbConfig authority. Pending authorities have no powers yet.
    pub authority: Signer<'info>,

    /// Global ArbConfig PDA.
    #[account(
        mut,
        seeds = [ARB_CONFIG_SEED],
        bump = arb_config.bump,
        constraint = authority.key() == arb_config.authority
            @ EpochError::UnauthorizedArbConfigAuthority,
    )]
    pub arb_config: Account<'info, ArbConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> Pubkey {
        Pubkey::new_from_array([byte; 32])
    }

    fn config() -> ArbConfig {
        ArbConfig {
            authority: key(1),
            pending_authority: key(6),
            wallet_a: key(2),
            wallet_b: key(3),
            pause_authority: key(4),
            pause_tripwire: key(5),
            pause_slots: 200,
            epoch_length_slots: 4_500,
            bump: 255,
            reserved: [0u8; 64],
        }
    }

    fn assert_error_name(result: Result<ResultingArbConfig>, expected: &str) {
        match result.expect_err("candidate should be rejected") {
            anchor_lang::error::Error::AnchorError(error) => {
                assert_eq!(error.error_name, expected)
            }
            other => panic!("expected Anchor error {expected}, got {other:?}"),
        }
    }

    #[test]
    fn validates_new_timing_against_unchanged_timing() {
        let args = UpdateArbConfigArgs {
            new_epoch_length_slots: Some(150),
            ..UpdateArbConfigArgs::default()
        };

        assert_error_name(
            validate_resulting_config(&config(), &args),
            "PauseSlotsNotBelowEpochLength",
        );
    }

    #[test]
    fn validates_new_wallet_against_unchanged_roles() {
        let args = UpdateArbConfigArgs {
            new_wallet_a: Some(key(4)),
            ..UpdateArbConfigArgs::default()
        };

        assert_error_name(
            validate_resulting_config(&config(), &args),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn allows_both_wallet_fields_to_converge() {
        let args = UpdateArbConfigArgs {
            new_wallet_a: Some(key(3)),
            ..UpdateArbConfigArgs::default()
        };

        let resulting = validate_resulting_config(&config(), &args).unwrap();
        assert_eq!(resulting.wallet_a, key(3));
        assert_eq!(resulting.wallet_b, key(3));
    }

    #[test]
    fn nonzero_new_authority_changes_pending_only() {
        let args = UpdateArbConfigArgs {
            new_authority: Some(key(7)),
            ..UpdateArbConfigArgs::default()
        };

        let resulting = validate_resulting_config(&config(), &args).unwrap();
        assert_eq!(resulting.authority, key(1));
        assert_eq!(resulting.pending_authority, key(7));
    }

    #[test]
    fn default_new_authority_cancels_pending_without_burning_authority() {
        let args = UpdateArbConfigArgs {
            new_authority: Some(Pubkey::default()),
            ..UpdateArbConfigArgs::default()
        };

        let resulting = validate_resulting_config(&config(), &args).unwrap();
        assert_eq!(resulting.authority, key(1));
        assert_eq!(resulting.pending_authority, Pubkey::default());
    }

    #[test]
    fn pending_candidate_is_checked_against_resulting_roles() {
        let args = UpdateArbConfigArgs {
            new_authority: Some(key(7)),
            new_pause_authority: Some(key(7)),
            ..UpdateArbConfigArgs::default()
        };

        assert_error_name(
            validate_resulting_config(&config(), &args),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn provided_zero_active_role_is_rejected() {
        let args = UpdateArbConfigArgs {
            new_pause_tripwire: Some(Pubkey::default()),
            ..UpdateArbConfigArgs::default()
        };

        assert_error_name(
            validate_resulting_config(&config(), &args),
            "ZeroPubkeyForbidden",
        );
    }

    #[test]
    fn absent_authority_update_preserves_pending_transfer() {
        let resulting =
            validate_resulting_config(&config(), &UpdateArbConfigArgs::default()).unwrap();

        assert_eq!(resulting.authority, key(1));
        assert_eq!(resulting.pending_authority, key(6));
    }
}
