//! Accept ArbConfig authority instruction.
//!
//! Completes the second step of an ArbConfig authority transfer only after the
//! pending key signs and remains distinct from every current active role.

use anchor_lang::prelude::*;

use crate::constants::ARB_CONFIG_SEED;
use crate::errors::EpochError;
use crate::events::ArbConfigUpdated;
use crate::state::{validate_role_overlap, ArbConfig};

/// Validate a pending authority acceptance in its required error order.
fn validate_acceptance(arb_config: &ArbConfig, signer: Pubkey) -> Result<Pubkey> {
    require!(
        arb_config.pending_authority != Pubkey::default(),
        EpochError::NoPendingAuthority
    );
    require!(
        signer == arb_config.pending_authority,
        EpochError::UnauthorizedPendingAuthority
    );
    validate_role_overlap(
        &signer,
        &arb_config.wallet_a,
        &arb_config.wallet_b,
        &arb_config.pause_authority,
        &arb_config.pause_tripwire,
    )?;

    Ok(signer)
}

/// Accept a pending ArbConfig authority transfer.
///
/// The current authority retains full update, overwrite, and cancellation
/// powers until this instruction succeeds. Pending transfers never expire;
/// their cancel/overwrite path is `update_arb_config`. Acceptance re-checks
/// the pending key against every currently stored role before changing the
/// active authority, and a missing pending key can never burn the authority.
///
/// Source: Centralized-Arb MVP Spec §14.6 entries 4, 5, and 6.
pub fn handler(ctx: Context<AcceptArbConfigAuthority>) -> Result<()> {
    let signer = ctx.accounts.new_authority.key();
    let arb_config = &mut ctx.accounts.arb_config;
    let accepted_authority = validate_acceptance(arb_config, signer)?;
    let slot = Clock::get()?.slot;

    arb_config.authority = accepted_authority;
    arb_config.pending_authority = Pubkey::default();

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

/// Accounts for `accept_arb_config_authority`.
#[derive(Accounts)]
pub struct AcceptArbConfigAuthority<'info> {
    /// Pending authority that must explicitly accept the transfer.
    pub new_authority: Signer<'info>,

    /// Global ArbConfig PDA.
    #[account(
        mut,
        seeds = [ARB_CONFIG_SEED],
        bump = arb_config.bump,
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
            pause_slots: 25,
            epoch_length_slots: 4_500,
            bump: 255,
            reserved: [0u8; 64],
        }
    }

    fn assert_error_name(result: Result<Pubkey>, expected: &str) {
        match result.expect_err("acceptance should be rejected") {
            anchor_lang::error::Error::AnchorError(error) => {
                assert_eq!(error.error_name, expected)
            }
            other => panic!("expected Anchor error {expected}, got {other:?}"),
        }
    }

    #[test]
    fn no_pending_authority_is_rejected_first() {
        let mut arb_config = config();
        arb_config.pending_authority = Pubkey::default();

        assert_error_name(
            validate_acceptance(&arb_config, Pubkey::default()),
            "NoPendingAuthority",
        );
    }

    #[test]
    fn signer_must_match_pending_authority() {
        assert_error_name(
            validate_acceptance(&config(), key(7)),
            "UnauthorizedPendingAuthority",
        );
    }

    #[test]
    fn pending_authority_is_rechecked_against_current_roles() {
        let mut arb_config = config();
        arb_config.wallet_a = arb_config.pending_authority;

        assert_error_name(
            validate_acceptance(&arb_config, key(6)),
            "RoleOverlapForbidden",
        );
    }

    #[test]
    fn valid_pending_signer_can_accept() {
        assert_eq!(validate_acceptance(&config(), key(6)).unwrap(), key(6));
    }
}
