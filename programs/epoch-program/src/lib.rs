//! Dr Fraudsworth Epoch Program
//!
//! VRF-driven tax regime coordination and Carnage Fund execution.
//!
//! The Epoch Program manages:
//! - Configurable epoch transitions with ORAO VRF v2
//! - Dynamic tax rates (1-4% low, 11-14% high)
//! - 75% flip probability between CRIME/FRAUD cheap sides
//! - ~4.3% Carnage trigger probability per epoch
//!
//! Source: Epoch_State_Machine_Spec.md

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod helpers;
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
pub static DRF_RELEASE_IDENTITY_EPOCH: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=devnet|program=64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY|tax=5szL392ooEi7ExCZj7ewxohk5aACnBGhSYnXrr1KGZfZ|amm=AG7QjhAoUecWK8iDDtzB4RzQY2HA7RgHbPeZimJSGwU1|staking=3wUWmbPUmsEzBmmCn9amftaLi1V7fai4bd3ANkyzT8HC|orao=VRFzZoJdhFWL8rkvu87LpKM3RbcVezpMEc6X5GVDr7y|network_state=5ER1oENnV4srxYdAynUfRzWeQCPQaqMiAp4VqyMbSqnK";
#[cfg(all(not(feature = "no-entrypoint"), not(feature = "devnet")))]
#[used]
#[no_mangle]
pub static DRF_RELEASE_IDENTITY_EPOCH: &[u8] = b"DRF_RELEASE_IDENTITY_V1|cluster=mainnet|program=4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2|tax=43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj|amm=5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR|staking=12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH|orao=VRFzZoJdhFWL8rkvu87LpKM3RbcVezpMEc6X5GVDr7y|network_state=5ER1oENnV4srxYdAynUfRzWeQCPQaqMiAp4VqyMbSqnK";

#[cfg(feature = "devnet")]
declare_id!("64yriwHLm9JW8tkeKaiURmV7KdGACFSVs1psRrXjcGdY");
#[cfg(not(feature = "devnet"))]
declare_id!("4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2");

#[program]
pub mod epoch_program {
    use super::*;

    /// Initialize the global epoch state.
    ///
    /// Called once at protocol deployment. Sets up genesis configuration
    /// with CRIME as the cheap side, 3% low tax, 14% high tax.
    ///
    /// # Arguments
    /// None - all values are hardcoded for genesis.
    ///
    /// # Errors
    /// - `AlreadyInitialized` if called more than once
    pub fn initialize_epoch_state(ctx: Context<InitializeEpochState>) -> Result<()> {
        instructions::initialize_epoch_state::handler(ctx)
    }

    /// Explicitly activate the reserved-byte safety carve on an existing,
    /// quiescent EpochState. This deterministic migration is permissionless,
    /// one-way, and required before any post-upgrade transition operation.
    pub fn activate_epoch_safety(ctx: Context<ActivateEpochSafety>) -> Result<()> {
        instructions::activate_epoch_safety::handler(ctx)
    }

    /// Trigger an epoch transition.
    ///
    /// Called by a configured arb wallet when the epoch boundary is reached,
    /// or permissionlessly once the grace window opens. Issues an ORAO
    /// `request_v2` CPI and binds the resulting request PDA for anti-reroll
    /// protection. Nothing needs bundling: ORAO inits the request itself.
    ///
    /// This is the first of TWO of our transactions:
    /// 1. TX 1: trigger_epoch_transition (requests randomness)
    /// 2. ORAO's fulfilment nodes land their own transaction; we send nothing
    /// 3. TX 2: consume_randomness
    ///
    /// # Accounts
    /// - `payer`: Arb wallet before grace; anyone after grace; receives the bounty
    /// - `epoch_state`: Global epoch state (mutated)
    /// - `arb_config`: Live epoch length, pause length, and trigger wallets
    /// - `carnage_sol_vault`: Carnage SOL vault PDA (funds bounty via invoke_signed)
    /// - `randomness_account`: the ORAO request PDA this epoch/attempt derives to
    /// - `network_state`, `treasury`, `vrf_program`: ORAO CPI accounts
    ///
    /// # Errors
    /// - `EpochBoundaryNotReached` if current slot hasn't passed next epoch boundary
    /// - `UnauthorizedTrigger` if a stranger calls before the grace boundary
    /// - `VrfAlreadyPending` if a VRF request is already in progress
    /// - `RandomnessAccountMismatch` if the passed account is not the derived PDA
    pub fn trigger_epoch_transition(ctx: Context<TriggerEpochTransition>) -> Result<()> {
        instructions::trigger_epoch_transition::handler(ctx)
    }

    /// Consume revealed VRF randomness and update taxes.
    ///
    /// Called once ORAO's fulfilment nodes have written the randomness
    /// (observed 1-3 s on devnet, ~36 s on mainnet). Verifies anti-reroll
    /// protection, reads the VRF bytes, derives tax rates.
    ///
    /// This is the second and last of our two transactions:
    /// 1. TX 1: trigger_epoch_transition (requests randomness)
    /// 2. ORAO's fulfilment nodes land their own transaction; we send nothing
    /// 3. TX 2: consume_randomness (this instruction)
    ///
    /// # Accounts
    /// - `caller`: Anyone
    /// - `epoch_state`: Global epoch state (mutated)
    /// - `randomness_account`: the SAME ORAO request PDA bound at trigger,
    ///   re-verified by stored key AND by seed re-derivation
    ///
    /// # Errors
    /// - `NoVrfPending` if no VRF request is pending
    /// - `RandomnessAccountMismatch` if account doesn't match bound account (anti-reroll)
    /// - `RandomnessParseError` if randomness account data is invalid
    /// - `RandomnessNotRevealed` if oracle hasn't revealed yet
    /// - `InsufficientRandomness` if less than 6 bytes revealed
    pub fn consume_randomness(ctx: Context<ConsumeRandomness>) -> Result<()> {
        instructions::consume_randomness::handler(ctx)
    }

    /// Retry VRF after timeout.
    ///
    /// Called permissionlessly if the oracle fails to fulfil within 300 slots
    /// (~2 min). Requests a fresh ORAO account at attempt+1, which derives a
    /// different PDA. Nothing needs bundling.
    ///
    /// This is a recovery mechanism to prevent protocol deadlock. If the original
    /// oracle fails to reveal, anyone can call this instruction with a new
    /// randomness account to restart the VRF process.
    ///
    /// # Accounts
    /// - `payer`: Anyone
    /// - `epoch_state`: Global epoch state (mutated)
    /// - `randomness_account`: the fresh ORAO request PDA for attempt+1
    /// - `network_state`, `treasury`, `vrf_program`, `system_program`: ORAO CPI
    ///
    /// # Errors
    /// - `NoVrfPending` if no VRF request is pending
    /// - `VrfTimeoutNotElapsed` if 300 slots haven't passed since original request
    /// - `RandomnessAccountMismatch` if the passed account is not the derived PDA
    /// - `RandomnessReplacementUnchanged` if it equals the pending account
    pub fn retry_epoch_vrf(ctx: Context<RetryEpochVrf>) -> Result<()> {
        instructions::retry_epoch_vrf::handler(ctx)
    }

    /// Resolve a VRF generation after its final attempt times out or its
    /// absolute lifetime is reached. Keeps the last confirmed taxes, finalizes
    /// Staking, schedules no Carnage, and clears pending state permissionlessly.
    pub fn terminal_vrf_fallback(ctx: Context<TerminalVrfFallback>) -> Result<()> {
        instructions::terminal_vrf_fallback::handler(ctx)
    }

    /// Initialize the Carnage Fund.
    ///
    /// Called once at protocol deployment. Creates the Carnage Fund state
    /// account and token vaults for CRIME and FRAUD.
    ///
    /// The SOL vault is a SystemAccount PDA that will hold native lamports
    /// from protocol fees. The token vaults are Token-2022 accounts that
    /// will hold purchased tokens before burning.
    ///
    /// # Accounts
    /// - `authority`: Deployer (pays for account creation)
    /// - `carnage_state`: Carnage Fund state PDA (created)
    /// - `sol_vault`: SOL vault PDA (SystemAccount)
    /// - `crime_vault`: CRIME token vault PDA (Token-2022, created)
    /// - `fraud_vault`: FRAUD token vault PDA (Token-2022, created)
    /// - `crime_mint`: CRIME token mint
    /// - `fraud_mint`: FRAUD token mint
    /// - `token_program`: Token-2022 program
    /// - `system_program`: System program
    ///
    /// # Errors
    /// - `CarnageAlreadyInitialized` if called more than once
    pub fn initialize_carnage_fund(ctx: Context<InitializeCarnageFund>) -> Result<()> {
        instructions::initialize_carnage_fund::handler(ctx)
    }

    /// Execute pending Carnage (fallback).
    ///
    /// Called permissionlessly when atomic Carnage execution failed.
    /// Must be called within 100 slots of the failure (carnage_deadline_slot).
    ///
    /// This instruction performs the same execution as atomic Carnage:
    /// 1. If holdings exist and action = Burn: burn tokens, then buy target
    /// 2. If holdings exist and action = Sell: sell tokens to SOL, then buy target
    /// 3. If no holdings: just buy target token
    ///
    /// All swaps are tax-exempt (0% tax, 1% LP fee only).
    ///
    /// # Accounts
    /// - `caller`: Anyone (permissionless)
    /// - `epoch_state`: Global epoch state (has pending flags)
    /// - `carnage_state`: Carnage Fund state (updated)
    /// - `sol_vault`: Carnage SOL vault
    ///
    /// # Errors
    /// - `NoCarnagePending` if no Carnage execution is pending
    /// - `CarnageDeadlineExpired` if current_slot > carnage_deadline_slot
    /// - `CarnageNotInitialized` if Carnage Fund not initialized
    ///
    /// Source: Carnage_Fund_Spec.md Section 13.3
    pub fn execute_carnage<'info>(
        ctx: Context<'_, '_, 'info, 'info, ExecuteCarnage<'info>>,
        generation: u32,
    ) -> Result<()> {
        instructions::execute_carnage::handler(ctx, generation)
    }

    /// Execute Carnage atomically (primary path).
    ///
    /// Called immediately after consume_randomness when Carnage is triggered.
    /// Typically bundled in the same transaction for MEV protection.
    ///
    /// This instruction executes the full Carnage flow:
    /// 1. If holdings exist and action = Burn: burn tokens, then buy target
    /// 2. If holdings exist and action = Sell: sell tokens to SOL via Tax::swap_exempt, then buy target
    /// 3. If no holdings: just buy target token via Tax::swap_exempt
    ///
    /// All swaps are tax-exempt (0% tax, 1% LP fee only) via Tax::swap_exempt.
    /// Swap amount is capped adaptively from the live SOL reserve and tax-pair friction.
    ///
    /// CRITICAL CPI DEPTH: This path reaches Solana's 4-level limit:
    ///   execute_carnage_atomic -> Tax::swap_exempt -> AMM::swap_sol_pool
    ///   -> Token-2022::transfer_checked -> Transfer Hook::execute
    ///
    /// # Accounts
    /// - `caller`: Anyone (permissionless when carnage_pending = true)
    /// - `epoch_state`: Global epoch state (has pending Carnage flags)
    /// - `carnage_state`: Carnage Fund state (updated with holdings/stats)
    /// - `carnage_signer`: PDA that signs Tax::swap_exempt calls
    /// - `sol_vault`: Carnage SOL vault (native lamports)
    /// - `carnage_wsol`: Carnage WSOL account for swap operations
    /// - `crime_vault`: Carnage CRIME vault (Token-2022)
    /// - `fraud_vault`: Carnage FRAUD vault (Token-2022)
    /// - `target_pool`: AMM pool for target token
    /// - `pool_vault_a/b`: Pool vaults
    /// - `mint_a/b`: Token mints
    /// - `tax_program`: Tax Program for swap_exempt CPI
    /// - `amm_program`: AMM Program (passed through to Tax)
    /// - `token_program_a/b`: Token programs
    /// - `system_program`: System program
    ///
    /// # Errors
    /// - `NoCarnagePending` if carnage_pending = false
    /// - `CarnageNotInitialized` if Carnage Fund not initialized
    /// - `InvalidCarnageTargetPool` if target pool doesn't match pending target
    /// - `Overflow` if statistics overflow
    ///
    /// Source: Carnage_Fund_Spec.md Sections 8-10, 13.2
    pub fn execute_carnage_atomic<'info>(
        ctx: Context<'_, '_, 'info, 'info, ExecuteCarnageAtomic<'info>>,
        generation: u32,
    ) -> Result<()> {
        instructions::execute_carnage_atomic::handler(ctx, generation)
    }

    /// Expire pending Carnage after deadline.
    ///
    /// Called permissionlessly after the 100-slot deadline has passed.
    /// Clears the pending Carnage state. SOL is retained in vault for
    /// the next Carnage trigger.
    ///
    /// This instruction does NOT execute Carnage - it simply clears the
    /// pending state so the protocol can continue. The accumulated SOL
    /// remains in the Carnage vault and will be used on the next trigger.
    ///
    /// # Accounts
    /// - `caller`: Anyone (permissionless)
    /// - `epoch_state`: Global epoch state (pending flags cleared)
    /// - `carnage_state`: Carnage Fund state (read for vault balance)
    /// - `sol_vault`: Carnage SOL vault (read for balance in event)
    ///
    /// # Errors
    /// - `NoCarnagePending` if no Carnage execution is pending
    /// - `CarnageDeadlineNotExpired` if current_slot <= carnage_deadline_slot
    ///
    /// Source: Carnage_Fund_Spec.md Section 13.4
    pub fn expire_carnage(ctx: Context<ExpireCarnage>, generation: u32) -> Result<()> {
        instructions::expire_carnage::handler(ctx, generation)
    }

    /// DEVNET ONLY: Force Carnage pending state for testing.
    ///
    /// Admin-gated test helper that sets carnage_pending on EpochState
    /// without waiting for a natural VRF trigger. Allows rapid testing
    /// of all Carnage execution paths (Burn, Sell, BuyOnly).
    ///
    /// MUST BE REMOVED BEFORE MAINNET DEPLOYMENT.
    ///
    /// # Arguments
    /// - `target`: 0 = CRIME, 1 = FRAUD
    /// - `action`: 0 = None (BuyOnly), 1 = Burn, 2 = Sell
    #[cfg(feature = "devnet")]
    pub fn force_carnage(ctx: Context<ForceCarnage>, target: u8, action: u8) -> Result<()> {
        instructions::force_carnage::handler(ctx, target, action)
    }

    /// Initialize the global ArbConfig PDA.
    ///
    /// The payer must be the deployed Epoch program's verified ProgramData
    /// upgrade authority and becomes `arb_config.authority`; authority is
    /// deliberately not accepted as an argument. Anchor's `init` constraint
    /// rejects re-initialization when the PDA already exists.
    ///
    /// Source: Centralized-Arb MVP Spec §3.1, §14.1 ruling 13, and §14.6
    /// entries 1, 5, and 6.
    pub fn initialize_arb_config(
        ctx: Context<InitializeArbConfig>,
        wallet_a: Pubkey,
        wallet_b: Pubkey,
        pause_slots: u64,
        epoch_length_slots: u64,
        pause_authority: Pubkey,
        pause_tripwire: Pubkey,
    ) -> Result<()> {
        instructions::initialize_arb_config::handler(
            ctx,
            wallet_a,
            wallet_b,
            pause_slots,
            epoch_length_slots,
            pause_authority,
            pause_tripwire,
        )
    }

    /// Update ArbConfig one-shot fields or initiate, overwrite, or cancel a
    /// pending two-step authority transfer.
    ///
    /// The current authority remains active until the pending signer accepts.
    /// A default `new_authority` cancels only the pending transfer and never
    /// writes the active authority.
    ///
    /// Source: Centralized-Arb MVP Spec §3.1 and §14.6 entries 4, 5, and 6.
    pub fn update_arb_config(
        ctx: Context<UpdateArbConfig>,
        args: UpdateArbConfigArgs,
    ) -> Result<()> {
        instructions::update_arb_config::handler(ctx, args)
    }

    /// Complete a two-step ArbConfig authority transfer.
    ///
    /// Only the nonzero pending key may accept. Its separation from every
    /// current active role is re-checked at acceptance time, after which the
    /// pending sentinel is cleared.
    ///
    /// Source: Centralized-Arb MVP Spec §14.6 entries 4, 5, and 6.
    pub fn accept_arb_config_authority(ctx: Context<AcceptArbConfigAuthority>) -> Result<()> {
        instructions::accept_arb_config_authority::handler(ctx)
    }

    /// Set the indefinite manual trading pause.
    ///
    /// The primary pause authority and the set-only tripwire are admitted.
    /// Redundant authorized calls succeed without emitting an event.
    pub fn set_trading_pause(ctx: Context<SetTradingPause>) -> Result<()> {
        instructions::set_trading_pause::handler(ctx)
    }

    /// Clear the indefinite manual trading pause.
    ///
    /// Only the primary pause authority is admitted. Redundant authorized
    /// calls succeed without emitting an event so rotate-and-clear batches are
    /// race safe.
    pub fn clear_trading_pause(ctx: Context<ClearTradingPause>) -> Result<()> {
        instructions::clear_trading_pause::handler(ctx)
    }
}

// ---------------------------------------------------------------------------
// IDL Verification Tests (CTG-02)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn force_carnage_excluded_from_non_devnet_idl() {
        // Read the IDL file generated by `anchor build` (without devnet feature).
        // If built with default features, force_carnage should NOT appear.
        let idl_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/idl/epoch_program.json"
        );
        let Ok(idl_content) = std::fs::read_to_string(idl_path) else {
            eprintln!("IDL file not found at {idl_path} -- skipping (run `anchor build` first)");
            return;
        };

        // In non-devnet builds, force_carnage should not appear in instructions.
        // Anchor IDL uses camelCase, so check both forms.
        if cfg!(not(feature = "devnet")) {
            assert!(
                !idl_content.contains("forceCarnage") && !idl_content.contains("force_carnage"),
                "force_carnage found in non-devnet IDL! The cfg gate may have been removed."
            );
        }
    }
}
