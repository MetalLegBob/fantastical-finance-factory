//! Canonical Staking finalization CPI shared by successful and degraded epoch
//! transitions.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};

use crate::constants::{STAKING_AUTHORITY_SEED, UPDATE_CUMULATIVE_DISCRIMINATOR};

pub fn finalize_staking_epoch<'info>(
    staking_authority: &AccountInfo<'info>,
    stake_pool: &AccountInfo<'info>,
    staking_program: &AccountInfo<'info>,
    staking_authority_bump: u8,
    epoch: u32,
) -> Result<()> {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&UPDATE_CUMULATIVE_DISCRIMINATOR);
    data.extend_from_slice(&epoch.to_le_bytes());

    let instruction = Instruction {
        program_id: *staking_program.key,
        accounts: vec![
            AccountMeta::new_readonly(*staking_authority.key, true),
            AccountMeta::new(*stake_pool.key, false),
        ],
        data,
    };
    let signer_seeds: &[&[u8]] = &[STAKING_AUTHORITY_SEED, &[staking_authority_bump]];

    invoke_signed(
        &instruction,
        &[
            staking_authority.clone(),
            stake_pool.clone(),
            staking_program.clone(),
        ],
        &[signer_seeds],
    )
    .map_err(Into::into)
}
