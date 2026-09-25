use anchor_lang::prelude::*;

/// Emitted when the whitelist authority is permanently burned.
///
/// After this event, no new whitelist entries can be added.
/// The whitelist becomes immutable.
///
/// Spec reference: Transfer_Hook_Spec.md Section 11
#[event]
pub struct AuthorityBurned {
    /// The pubkey that burned the authority (must have been the authority).
    pub burned_by: Pubkey,
    /// Unix timestamp when the authority was burned.
    pub timestamp: i64,
}

/// Emitted when a new address is added to the whitelist.
///
/// Provides audit trail for all whitelist additions.
///
/// Spec reference: Transfer_Hook_Spec.md Section 11
#[event]
pub struct AddressWhitelisted {
    /// The address that was whitelisted.
    pub address: Pubkey,
    /// The authority pubkey that added this entry.
    pub added_by: Pubkey,
    /// Unix timestamp when the entry was created.
    pub timestamp: i64,
}

/// Emitted when ExtraAccountMetaList is initialized for a mint.
///
/// This signifies the mint is now configured for transfer hook invocation.
/// Token-2022 can now resolve whitelist PDAs at transfer time.
///
/// Spec reference: Transfer_Hook_Spec.md Section 8
#[event]
pub struct ExtraAccountMetaListInitialized {
    /// The mint for which ExtraAccountMetaList was created.
    pub mint: Pubkey,
}

// NOTE: TransferBlocked event will be added in Phase 17 when implementing
// the transfer_hook instruction. It requires source, destination, amount, reason
// fields which are only relevant in transfer validation context.
