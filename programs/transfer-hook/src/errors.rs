use anchor_lang::prelude::*;

/// Error codes for the Transfer Hook program.
///
/// Spec reference: Transfer_Hook_Spec.md Section 10
#[error_code]
pub enum TransferHookError {
    // --- Transfer validation errors (used by transfer_hook instruction, Phase 17) ---
    /// Neither the source nor destination token account is whitelisted.
    /// Transfers require at least one party to be a protocol-controlled address.
    #[msg("Neither source nor destination is whitelisted")]
    NoWhitelistedParty,

    /// Zero amount transfers are explicitly blocked.
    /// These waste compute and indicate a client bug or attack vector.
    #[msg("Zero amount transfers are not allowed")]
    ZeroAmountTransfer,

    // --- Authority/whitelist management errors (used by admin instructions, Phase 15) ---
    /// The signer is not the stored whitelist authority.
    /// Only the authority can add whitelist entries or burn authority.
    #[msg("Unauthorized: signer is not the authority")]
    Unauthorized,

    /// Attempted to modify whitelist after authority was burned.
    /// Once burned, the whitelist is permanently immutable.
    #[msg("Whitelist authority has already been burned")]
    AuthorityAlreadyBurned,

    /// Attempted to add an address that already has a whitelist entry PDA.
    /// Each address can only be whitelisted once.
    #[msg("Address is already whitelisted")]
    AlreadyWhitelisted,

    // --- PDA validation errors (used by transfer_hook instruction, Phase 17) ---
    /// The whitelist PDA passed does not match expected derivation.
    /// Prevents attackers from passing fake whitelist PDAs.
    #[msg("Invalid whitelist PDA derivation")]
    InvalidWhitelistPDA,

    /// Transfer hook invoked directly, not through Token-2022 transfer.
    /// This is a security check - hooks should only be called by Token-2022 during transfer_checked.
    #[msg("Transfer hook must be invoked through Token-2022 transfer")]
    DirectInvocationNotAllowed,

    /// Mint is not owned by Token-2022 program.
    /// Defense-in-depth: validates mint.owner == token_2022_program_id even though
    /// ExtraAccountMetaList provides implicit mint validation (only initialized mints can invoke hook).
    #[msg("Mint is not a valid Token-2022 mint")]
    InvalidMint,

    // --- ExtraAccountMetaList errors (used by initialize_extra_account_meta_list, Phase 16) ---
    /// Mint's transfer hook extension does not point to this program.
    /// This prevents ExtraAccountMetaList creation for mints that would never invoke our hook.
    #[msg("Mint's transfer hook extension does not point to this program")]
    InvalidTransferHook,

    /// Mint is not owned by the Token-2022 program.
    /// ExtraAccountMetaList can only be created for Token-2022 mints with transfer hook extension.
    #[msg("Mint is not a Token-2022 mint")]
    NotToken2022Mint,
}
