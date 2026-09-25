use anchor_lang::prelude::*;
use spl_discriminator::SplDiscriminate;
use spl_transfer_hook_interface::instruction::{
    ExecuteInstruction, InitializeExtraAccountMetaListInstruction,
};

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

use instructions::*;

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Dr Fraudsworth's Finance Factory",
    project_url: "https://fraudsworth.fun",
    contacts: "email:drfraudsworth@gmail.com,twitter:@fraudsworth",
    policy: "https://fraudsworth.fun/docs/security/security-policy",
    preferred_languages: "en",
    auditors: "Internal audits: SOS, BOK, VulnHunter (v1.3)",
    expiry: "2027-03-20"
}

#[cfg(all(not(feature = "no-entrypoint"), feature = "devnet"))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_TRANSFER_HOOK: &[u8] =
    b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=3FQXiMUNfDJcVm4tpyKhTjeDMM1p4A8weF33UET89d5P";
#[cfg(all(not(feature = "no-entrypoint"), not(feature = "devnet")))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_TRANSFER_HOOK: &[u8] =
    b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=CiQPQrmQh6BPhb9k7dFnsEs5gKPgdrvNKFc5xie5xVGd";

#[cfg(feature = "devnet")]
declare_id!("3FQXiMUNfDJcVm4tpyKhTjeDMM1p4A8weF33UET89d5P");
#[cfg(not(feature = "devnet"))]
declare_id!("CiQPQrmQh6BPhb9k7dFnsEs5gKPgdrvNKFc5xie5xVGd");

#[program]
pub mod transfer_hook {
    use super::*;

    /// Initialize the whitelist authority account.
    ///
    /// Creates WhitelistAuthority PDA with the transaction signer as authority.
    /// Can only be called once (Anchor init constraint prevents reinitialization).
    ///
    /// Spec reference: Transfer_Hook_Spec.md Section 7.1
    pub fn initialize_authority(ctx: Context<InitializeAuthority>) -> Result<()> {
        instructions::initialize_authority::handler(ctx)
    }

    /// Add an address to the whitelist.
    ///
    /// Creates WhitelistEntry PDA for the given address. Only callable by
    /// the whitelist authority while authority is not burned.
    ///
    /// # Errors
    /// - Unauthorized: Signer is not the authority
    /// - AuthorityAlreadyBurned: Authority has been burned
    /// - (Anchor init failure): Address already whitelisted (PDA exists)
    ///
    /// Spec reference: Transfer_Hook_Spec.md Section 7.2
    pub fn add_whitelist_entry(ctx: Context<AddWhitelistEntry>) -> Result<()> {
        instructions::add_whitelist_entry::handler(ctx)
    }

    /// Transfer the whitelist authority to a new pubkey (e.g., Squads multisig vault).
    /// Only the current authority can call this. new_authority must not be Pubkey::default().
    pub fn transfer_authority(
        ctx: Context<TransferAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::transfer_authority::handler(ctx, new_authority)
    }

    /// Permanently burn the whitelist authority.
    ///
    /// Sets authority to None, making the whitelist immutable. This is
    /// idempotent: calling on an already-burned authority succeeds silently.
    ///
    /// # Errors
    /// - Unauthorized: Signer is not the authority (when authority exists)
    ///
    /// # Events
    /// - AuthorityBurned: Emitted on successful burn (not on idempotent call)
    ///
    /// Spec reference: Transfer_Hook_Spec.md Section 6.3, 7.3
    pub fn burn_authority(ctx: Context<BurnAuthority>) -> Result<()> {
        instructions::burn_authority::handler(ctx)
    }

    /// Initialize ExtraAccountMetaList for a mint.
    ///
    /// Creates the PDA that Token-2022 uses to resolve whitelist accounts
    /// at transfer time. Must be called once per mint before transfers.
    ///
    /// # Requirements
    /// - Mint must be Token-2022 with TransferHook extension pointing to this program
    /// - Authority must not be burned
    /// - ExtraAccountMetaList must not already exist for this mint
    ///
    /// # Events
    /// - ExtraAccountMetaListInitialized: Emitted on successful initialization
    ///
    /// Spec reference: Transfer_Hook_Spec.md Section 8
    ///
    /// Note: Uses SPL discriminator for Token-2022 transfer hook interface compatibility.
    #[instruction(discriminator = InitializeExtraAccountMetaListInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn initialize_extra_account_meta_list(
        ctx: Context<InitializeExtraAccountMetaList>,
    ) -> Result<()> {
        instructions::initialize_extra_account_meta_list::handler(ctx)
    }

    /// Transfer hook invoked by Token-2022 during transfer_checked.
    ///
    /// Validates that at least one party (source or destination) is whitelisted.
    /// Rejects zero-amount transfers and direct hook invocations.
    ///
    /// # Account Indices (SPL Transfer Hook Spec)
    /// - 0: source_token_account
    /// - 1: mint
    /// - 2: destination_token_account
    /// - 3: owner/authority
    /// - 4: extra_account_meta_list
    /// - 5: whitelist_source (resolved from ExtraAccountMetaList)
    /// - 6: whitelist_destination (resolved from ExtraAccountMetaList)
    ///
    /// # Errors
    /// - ZeroAmountTransfer: Amount is zero
    /// - InvalidMint: Mint not owned by Token-2022 (defense-in-depth)
    /// - DirectInvocationNotAllowed: Not called from Token-2022 transfer
    /// - NoWhitelistedParty: Neither source nor destination is whitelisted
    ///
    /// Spec reference: Transfer_Hook_Spec.md Section 7.4
    #[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn transfer_hook(ctx: Context<TransferHook>, amount: u64) -> Result<()> {
        instructions::transfer_hook::handler(ctx, amount)
    }
}
