//! Wallet-gated withdrawal from per-quote-asset sweep custody.
//!
//! Only the two wallets currently registered in the Epoch Program's canonical
//! ArbConfig may sign. The destination owner is separately allowlisted to the
//! two wallets or the ArbConfig authority, and payout always lands in that
//! owner's canonical ATA. This ops lane is deliberately not pause-gated.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::constants::{epoch_program_id, ARB_CONFIG_SEED, SWEEP_AUTHORITY_SEED};
use crate::errors::TaxError;
use crate::events::SweepWithdrawn;
use crate::state::ArbConfig;

/// Move an exact amount, or the full balance when `amount == 0`, from a
/// canonical sweep ATA to an allowlisted owner's canonical ATA.
pub fn handler(ctx: Context<WithdrawSweep>, amount: u64) -> Result<()> {
    // The requested withdrawal amount is an exact custody obligation. A
    // transfer-fee mint would silently under-credit the destination.
    amm::helpers::mint_policy::validate_amount_preserving_mint(
        &ctx.accounts.quote_mint.to_account_info(),
    )
    .map_err(|_| error!(TaxError::UnsupportedTransferFee))?;

    // GAP-1: validate the cross-program ArbConfig owner before deserializing
    // the frozen Tax-local mirror, then admit the two registered wallets only.
    require_keys_eq!(
        *ctx.accounts.arb_config.owner,
        epoch_program_id(),
        TaxError::InvalidArbConfig
    );
    let wallet = ctx.accounts.wallet.key();
    let (config_authority, wallet_a, wallet_b) = {
        let data = ctx.accounts.arb_config.try_borrow_data()?;
        let config = ArbConfig::try_deserialize(&mut &data[..])
            .map_err(|_| error!(TaxError::InvalidArbConfig))?;
        (config.authority, config.wallet_a, config.wallet_b)
    };
    require!(
        wallet == wallet_a || wallet == wallet_b,
        TaxError::UnauthorizedArbWallet
    );

    // Authority is an allowed payout owner, never an accepted signer here.
    let destination_owner = ctx.accounts.destination_owner.key();
    require!(
        destination_owner == wallet_a
            || destination_owner == wallet_b
            || destination_owner == config_authority,
        TaxError::InvalidSweepDestination
    );

    let balance = ctx.accounts.sweep_ata.amount;
    if amount > balance {
        return err!(TaxError::AmountExceedsSweepBalance);
    }
    let to_move = if amount == 0 { balance } else { amount };

    // Empty full-sweep loops are a silent success: no transfer and no event.
    if to_move == 0 {
        return Ok(());
    }

    let sweep_authority_seeds: &[&[u8]] = &[SWEEP_AUTHORITY_SEED, &[ctx.bumps.sweep_authority]];
    token_interface::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.sweep_ata.to_account_info(),
                mint: ctx.accounts.quote_mint.to_account_info(),
                to: ctx.accounts.destination_ata.to_account_info(),
                authority: ctx.accounts.sweep_authority.to_account_info(),
            },
            &[sweep_authority_seeds],
        ),
        to_move,
        ctx.accounts.quote_mint.decimals,
    )?;

    ctx.accounts.sweep_ata.reload()?;
    emit!(SweepWithdrawn {
        quote_mint: ctx.accounts.quote_mint.key(),
        destination_owner,
        wallet,
        amount: to_move,
        remaining_balance: ctx.accounts.sweep_ata.amount,
        slot: Clock::get()?.slot,
    });

    Ok(())
}

/// Accounts for the only exit from sweep custody.
///
/// Declaration order keeps the caller and canonical ArbConfig at the front of
/// the IDL. Anchor evaluates `init_if_needed` before the handler allowlist; if
/// that allowlist rejects, transaction rollback also reverts ATA creation and
/// caller rent, so an invalid owner cannot grief the caller into paying rent.
#[derive(Accounts)]
pub struct WithdrawSweep<'info> {
    /// Authorized ArbConfig wallet and payer for a missing destination ATA.
    #[account(mut)]
    pub wallet: Signer<'info>,

    /// Epoch Program's canonical ArbConfig PDA.
    /// CHECK: owner, discriminator, and fields are validated in the handler.
    #[account(
        seeds = [ARB_CONFIG_SEED],
        bump,
        seeds::program = epoch_program_id(),
    )]
    pub arb_config: AccountInfo<'info>,

    /// Tax Program PDA that owns every per-quote sweep ATA.
    /// CHECK: constrained by the canonical PDA seeds and used as transfer signer.
    #[account(
        seeds = [SWEEP_AUTHORITY_SEED],
        bump,
    )]
    pub sweep_authority: AccountInfo<'info>,

    /// Quote asset held in sweep custody (SPL Token or Token-2022).
    pub quote_mint: InterfaceAccount<'info, Mint>,

    /// Canonical sweep-authority ATA for this quote mint.
    #[account(
        mut,
        associated_token::mint = quote_mint,
        associated_token::authority = sweep_authority,
        associated_token::token_program = token_program,
    )]
    pub sweep_ata: InterfaceAccount<'info, TokenAccount>,

    /// Destination owner, checked against wallet A, wallet B, and authority.
    /// CHECK: handler allowlist is the sole admission rule.
    pub destination_owner: AccountInfo<'info>,

    /// Canonical destination ATA, created idempotently with caller-paid rent.
    #[account(
        init_if_needed,
        payer = wallet,
        associated_token::mint = quote_mint,
        associated_token::authority = destination_owner,
        associated_token::token_program = token_program,
    )]
    pub destination_ata: InterfaceAccount<'info, TokenAccount>,

    /// Token program selected by the quote mint (SPL Token or Token-2022).
    pub token_program: Interface<'info, TokenInterface>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}
