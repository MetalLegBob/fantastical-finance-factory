//! Wallet-gated SOL distribution through the frozen deployed 71/24/5 split.
//!
//! The caller supplies an exact nonzero lamport amount directly from its
//! system account. There is no escrow hop, minimum batch size, timing window,
//! or pause gate; cadence remains bot policy.

use anchor_lang::prelude::*;

use crate::constants::{
    epoch_program_id, staking_program_id, treasury_pubkey, ARB_CONFIG_SEED, CARNAGE_SOL_VAULT_SEED,
    ESCROW_VAULT_SEED, STAKE_POOL_SEED, TAX_AUTHORITY_SEED,
};
use crate::errors::TaxError;
use crate::events::SweptDistributed;
use crate::helpers::split::{execute_split_distribution, SplitDistributionAccounts};
use crate::state::{ArbConfig, EpochState};

/// Distribute an exact caller-provided SOL amount through the deployed split.
pub fn handler(ctx: Context<DistributeSwept>, lamports: u64) -> Result<()> {
    // GAP-1: validate owner and deserialize the frozen Tax-local ArbConfig
    // mirror before admitting either registered operations wallet.
    require_keys_eq!(
        *ctx.accounts.arb_config.owner,
        epoch_program_id(),
        TaxError::InvalidArbConfig
    );
    let wallet = ctx.accounts.wallet.key();
    let (wallet_a, wallet_b) = {
        let data = ctx.accounts.arb_config.try_borrow_data()?;
        let config = ArbConfig::try_deserialize(&mut &data[..])
            .map_err(|_| error!(TaxError::InvalidArbConfig))?;
        (config.wallet_a, config.wallet_b)
    };
    require!(
        wallet == wallet_a || wallet == wallet_b,
        TaxError::UnauthorizedArbWallet
    );

    // Unlike withdraw_sweep(0), zero cannot mean the caller's full SOL
    // balance because that balance is also the wallet's operating gas reserve.
    require!(lamports > 0, TaxError::ZeroDistribution);

    // EpochState is read only for accounting context. This ops lane deliberately
    // omits both manual-pause and epoch-pause gates.
    require_keys_eq!(
        *ctx.accounts.epoch_state.owner,
        epoch_program_id(),
        TaxError::InvalidEpochState
    );
    let epoch_state = {
        let data = ctx.accounts.epoch_state.try_borrow_data()?;
        let mut data_slice: &[u8] = &data;
        EpochState::try_deserialize(&mut data_slice)
            .map_err(|_| error!(TaxError::InvalidEpochState))?
    };
    require!(epoch_state.initialized, TaxError::InvalidEpochState);

    // Frozen helper call site #3. The wallet signed the transaction, so it is
    // the direct lamport source and requires no program-derived signer seeds.
    let (staking_amount, carnage_amount, treasury_amount) = execute_split_distribution(
        &SplitDistributionAccounts {
            source: &ctx.accounts.wallet.to_account_info(),
            source_signer_seeds: None,
            staking_escrow: &ctx.accounts.staking_escrow,
            stake_pool: &ctx.accounts.stake_pool,
            tax_authority: &ctx.accounts.tax_authority,
            tax_authority_bump: ctx.bumps.tax_authority,
            carnage_vault: &ctx.accounts.carnage_vault,
            treasury: &ctx.accounts.treasury,
            staking_program: &ctx.accounts.staking_program,
            system_program: &ctx.accounts.system_program.to_account_info(),
        },
        lamports,
    )?;

    emit!(SweptDistributed {
        caller: wallet,
        total: lamports,
        staking_amount,
        carnage_amount,
        treasury_amount,
        epoch: epoch_state.current_epoch,
        slot: Clock::get()?.slot,
    });

    Ok(())
}

/// Accounts for swept-SOL distribution.
#[derive(Accounts)]
pub struct DistributeSwept<'info> {
    /// Authorized ArbConfig wallet and direct lamport source.
    #[account(mut)]
    pub wallet: Signer<'info>,

    /// Epoch Program's canonical ArbConfig PDA.
    /// CHECK: owner, discriminator, and wallet fields are checked in handler.
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub arb_config: AccountInfo<'info>,

    /// Read-only accounting source for the emitted epoch number.
    /// CHECK: owner, discriminator, and initialized flag are checked in handler.
    pub epoch_state: AccountInfo<'info>,

    /// Staking Program escrow - receives 71% of tax (native SOL)
    /// CHECK: PDA derived from Staking Program seeds
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED],
        bump,
        seeds::program = staking_program_id(),
        constraint = true @ TaxError::InvalidStakingEscrow,
    )]
    pub staking_escrow: AccountInfo<'info>,

    /// Staking Program's StakePool PDA - updated by deposit_rewards CPI
    /// CHECK: PDA validated by Staking Program via seeds constraint
    #[account(
        mut,
        seeds = [STAKE_POOL_SEED],
        bump,
        seeds::program = staking_program_id(),
    )]
    pub stake_pool: AccountInfo<'info>,

    /// Tax Program's tax_authority PDA - signs Staking Program CPI
    /// CHECK: PDA derived from seeds, used as signer for deposit_rewards CPI
    #[account(
        seeds = [TAX_AUTHORITY_SEED],
        bump,
    )]
    pub tax_authority: AccountInfo<'info>,

    /// Carnage Fund vault - receives 24% of tax (native SOL)
    /// CHECK: PDA derived from Epoch Program seeds
    #[account(
        mut,
        seeds = [CARNAGE_SOL_VAULT_SEED],
        bump,
        seeds::program = epoch_program_id(),
        constraint = true @ TaxError::InvalidCarnageVault,
    )]
    pub carnage_vault: AccountInfo<'info>,

    /// Protocol treasury - receives 5% of tax (native SOL)
    /// CHECK: Address validated against known treasury pubkey
    #[account(
        mut,
        address = treasury_pubkey() @ TaxError::InvalidTreasury,
    )]
    pub treasury: AccountInfo<'info>,

    /// Staking Program for deposit_rewards CPI
    /// CHECK: Program ID validated in constants.rs staking_program_id()
    #[account(address = staking_program_id() @ TaxError::InvalidStakingProgram)]
    pub staking_program: AccountInfo<'info>,

    /// System program for the frozen helper's native SOL transfers.
    pub system_program: Program<'info, System>,
}
