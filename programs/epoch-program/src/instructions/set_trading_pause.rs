//! Set the manual trading pause.
//!
//! The dedicated cold authority and hot tripwire may set the pause. Clearing
//! lives in a separate instruction with a narrower authorization surface.

use anchor_lang::prelude::*;

use crate::constants::{ARB_CONFIG_SEED, EPOCH_STATE_SEED};
use crate::errors::EpochError;
use crate::events::TradingPauseChanged;
use crate::state::{ArbConfig, EpochState};

fn is_authorized_setter(signer: Pubkey, pause_authority: Pubkey, pause_tripwire: Pubkey) -> bool {
    signer == pause_authority || signer == pause_tripwire
}

fn pause_set_changes_state(trading_paused: bool) -> bool {
    !trading_paused
}

/// Set the indefinite manual trading pause.
///
/// Authorized emergency retries are idempotent: an already-paused state
/// returns success before any event is emitted. Who set a real transition and
/// when it happened live only in `TradingPauseChanged`, not in EpochState.
///
/// Source: Centralized-Arb MVP Spec §14.6 entries 2 and 3.
pub fn handler(ctx: Context<SetTradingPause>) -> Result<()> {
    let signer = ctx.accounts.signer.key();
    let arb_config = &ctx.accounts.arb_config;
    require!(
        is_authorized_setter(
            signer,
            arb_config.pause_authority,
            arb_config.pause_tripwire,
        ),
        EpochError::UnauthorizedPauseSet
    );

    let epoch_state = &mut ctx.accounts.epoch_state;
    if !pause_set_changes_state(epoch_state.trading_paused) {
        return Ok(());
    }

    let slot = Clock::get()?.slot;
    epoch_state.trading_paused = true;
    emit!(TradingPauseChanged {
        paused: true,
        setter: signer,
        slot,
    });

    Ok(())
}

/// Accounts for `set_trading_pause`.
#[derive(Accounts)]
pub struct SetTradingPause<'info> {
    /// Primary pause authority or set-only tripwire.
    pub signer: Signer<'info>,

    /// Global ArbConfig PDA, read only.
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump = arb_config.bump,
    )]
    pub arb_config: Account<'info, ArbConfig>,

    /// Global EpochState PDA whose flag is changed.
    #[account(
        mut,
        seeds = [EPOCH_STATE_SEED],
        bump = epoch_state.bump,
        constraint = epoch_state.initialized @ EpochError::NotInitialized,
    )]
    pub epoch_state: Account<'info, EpochState>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> Pubkey {
        Pubkey::new_from_array([byte; 32])
    }

    #[test]
    fn primary_authority_and_tripwire_can_set() {
        assert!(is_authorized_setter(key(1), key(1), key(2)));
        assert!(is_authorized_setter(key(2), key(1), key(2)));
    }

    #[test]
    fn unrelated_signer_cannot_set() {
        assert!(!is_authorized_setter(key(3), key(1), key(2)));
    }

    #[test]
    fn only_unpaused_state_requires_a_set_transition() {
        assert!(pause_set_changes_state(false));
        assert!(!pause_set_changes_state(true));
    }
}
