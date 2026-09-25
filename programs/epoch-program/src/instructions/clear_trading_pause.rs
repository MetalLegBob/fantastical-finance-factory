//! Clear the manual trading pause.
//!
//! This instruction deliberately exposes only the primary cold authority's
//! clearing power.

use anchor_lang::prelude::*;

use crate::constants::{ARB_CONFIG_SEED, EPOCH_STATE_SEED};
use crate::errors::EpochError;
use crate::events::TradingPauseChanged;
use crate::state::{ArbConfig, EpochState};

fn is_authorized_clearer(signer: Pubkey, pause_authority: Pubkey) -> bool {
    signer == pause_authority
}

fn pause_clear_changes_state(trading_paused: bool) -> bool {
    trading_paused
}

/// Clear the indefinite manual trading pause.
///
/// An already-clear state returns success before any event is emitted. This
/// no-op behavior is load-bearing for the §14.6 entry 3 recovery path: a
/// batched rotate-primary-authority-then-clear proposal cannot be aborted by a
/// benign race that clears the flag first.
pub fn handler(ctx: Context<ClearTradingPause>) -> Result<()> {
    let signer = ctx.accounts.signer.key();
    require!(
        is_authorized_clearer(signer, ctx.accounts.arb_config.pause_authority),
        EpochError::UnauthorizedPauseClear
    );

    let epoch_state = &mut ctx.accounts.epoch_state;
    if !pause_clear_changes_state(epoch_state.trading_paused) {
        return Ok(());
    }

    let slot = Clock::get()?.slot;
    epoch_state.trading_paused = false;
    emit!(TradingPauseChanged {
        paused: false,
        setter: signer,
        slot,
    });

    Ok(())
}

/// Accounts for `clear_trading_pause`.
#[derive(Accounts)]
pub struct ClearTradingPause<'info> {
    /// Primary pause authority; no other role has clearing power.
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
    fn primary_authority_can_clear() {
        assert!(is_authorized_clearer(key(1), key(1)));
    }

    #[test]
    fn unrelated_signer_cannot_clear() {
        assert!(!is_authorized_clearer(key(2), key(1)));
    }

    #[test]
    fn only_paused_state_requires_a_clear_transition() {
        assert!(!pause_clear_changes_state(false));
        assert!(pause_clear_changes_state(true));
    }
}
