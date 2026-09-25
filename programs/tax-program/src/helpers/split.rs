//! THE sacred 71/24/5 execution wrapper. One file = one sacred thing.
//!
//! Interface FROZEN for Phase 168's distribute_swept (third call site) — any
//! post-167 edit requires a full parity-differential re-run + flag to mlbob.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    system_instruction::transfer,
};

use crate::constants::{treasury_pubkey, DEPOSIT_REWARDS_DISCRIMINATOR, TAX_AUTHORITY_SEED};
use crate::errors::TaxError;
use crate::helpers::tax_math::split_distribution;

pub struct SplitDistributionAccounts<'a, 'info> {
    /// Lamport source: sell = swap_authority PDA; buy = user (tx signer); 168 = caller wallet.
    pub source: &'a AccountInfo<'info>,
    /// Some(seeds) when source is a program PDA; None when source signed the transaction.
    pub source_signer_seeds: Option<&'a [&'a [u8]]>,
    pub staking_escrow: &'a AccountInfo<'info>,
    pub stake_pool: &'a AccountInfo<'info>,
    pub tax_authority: &'a AccountInfo<'info>,
    pub tax_authority_bump: u8,
    pub carnage_vault: &'a AccountInfo<'info>,
    pub treasury: &'a AccountInfo<'info>,
    pub staking_program: &'a AccountInfo<'info>,
    pub system_program: &'a AccountInfo<'info>,
}

pub fn execute_split_distribution(
    accts: &SplitDistributionAccounts,
    tax_amount: u64,
) -> Result<(u64, u64, u64)> {
    require_keys_eq!(
        accts.treasury.key(),
        treasury_pubkey(),
        TaxError::InvalidTreasury
    );

    let (staking_portion, carnage_portion, treasury_portion) =
        split_distribution(tax_amount).ok_or(error!(TaxError::TaxOverflow))?;

    let seeds_holder: [&[&[u8]]; 1];
    let source_signers: &[&[&[u8]]] = match accts.source_signer_seeds {
        Some(s) => {
            seeds_holder = [s];
            &seeds_holder
        }
        None => &[],
    };

    if staking_portion > 0 {
        invoke_signed(
            &transfer(accts.source.key, accts.staking_escrow.key, staking_portion),
            &[
                accts.source.clone(),
                accts.staking_escrow.clone(),
                accts.system_program.clone(),
            ],
            source_signers,
        )?;

        // CPI to Staking Program's deposit_rewards to update pending_rewards counter.
        // The SOL is already in escrow; this just updates the state.
        let tax_authority_seeds: &[&[u8]] = &[TAX_AUTHORITY_SEED, &[accts.tax_authority_bump]];

        // Build deposit_rewards instruction data: discriminator (8) + amount (8)
        let mut deposit_ix_data = Vec::with_capacity(16);
        deposit_ix_data.extend_from_slice(&DEPOSIT_REWARDS_DISCRIMINATOR);
        deposit_ix_data.extend_from_slice(&staking_portion.to_le_bytes());

        // Build account metas for deposit_rewards:
        // Order matches Staking's DepositRewards struct: tax_authority, stake_pool, escrow_vault
        let deposit_accounts = vec![
            AccountMeta::new_readonly(accts.tax_authority.key(), true), // signer
            AccountMeta::new(accts.stake_pool.key(), false),
            AccountMeta::new_readonly(accts.staking_escrow.key(), false), // escrow_vault (balance reconciliation)
        ];

        let deposit_ix = Instruction {
            program_id: accts.staking_program.key(),
            accounts: deposit_accounts,
            data: deposit_ix_data,
        };

        invoke_signed(
            &deposit_ix,
            &[
                accts.tax_authority.clone(),
                accts.stake_pool.clone(),
                accts.staking_escrow.clone(),
                accts.staking_program.clone(),
            ],
            &[tax_authority_seeds],
        )?;
    }

    if carnage_portion > 0 {
        invoke_signed(
            &transfer(accts.source.key, accts.carnage_vault.key, carnage_portion),
            &[
                accts.source.clone(),
                accts.carnage_vault.clone(),
                accts.system_program.clone(),
            ],
            source_signers,
        )?;
    }

    if treasury_portion > 0 {
        invoke_signed(
            &transfer(accts.source.key, accts.treasury.key, treasury_portion),
            &[
                accts.source.clone(),
                accts.treasury.clone(),
                accts.system_program.clone(),
            ],
            source_signers,
        )?;
    }

    Ok((staking_portion, carnage_portion, treasury_portion))
}
