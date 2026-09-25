//! Raw byte reader for AMM PoolState fields.
//!
//! Reads fields from a PoolState AccountInfo without importing the AMM crate.
//! Uses known byte offsets verified against the AMM PoolState struct layout.
//! This avoids cross-crate coupling.
//!
//! Identical approach to epoch-program/src/instructions/execute_carnage_atomic.rs
//! (lines 930-956), adapted for Tax Program error types.
//!
//! PoolState byte layout (from AMM pool.rs):
//!   [0..8]     Anchor discriminator
//!   [8]        pool_type (1 byte)
//!   [9..41]    mint_a (Pubkey, 32 bytes)
//!   [41..73]   mint_b (Pubkey, 32 bytes)
//!   [73..105]  vault_a (Pubkey, 32 bytes)
//!   [105..137] vault_b (Pubkey, 32 bytes)
//!   [137..145] reserve_a (u64, 8 bytes)
//!   [145..153] reserve_b (u64, 8 bytes)
//!
//! Source: Phase 49 (SEC-10), 49-RESEARCH.md Pattern 2

use crate::constants::amm_program_id;
use crate::errors::TaxError;
use anchor_lang::prelude::*;

/// Return NATIVE_MINT address (So11111111111111111111111111111111111111112).
/// Hardcoded to avoid pulling in spl_token as a dependency just for this constant.
/// This is the canonical WSOL mint used by all SOL pools.
pub(crate) fn native_mint() -> Pubkey {
    use std::str::FromStr;
    Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap()
}

/// Read pool.mint_a (bytes [9..41]) with the same DEF-01 owner check as read_pool_reserves.
/// Phase 167 (TAX-02): the buy lane's WSOL-quote gate — must be mint_a == WSOL SPECIFICALLY
/// (a reversed pool with WSOL as mint_b is still un-tradeable by the hardcoded-AtoB lane).
pub fn read_pool_mint_a(pool_account: &AccountInfo) -> Result<Pubkey> {
    require!(
        *pool_account.owner == amm_program_id(),
        TaxError::InvalidPoolOwner
    );

    let data = pool_account.data.borrow();
    require!(data.len() >= 41, TaxError::InvalidPoolType);

    let mint_a_bytes: [u8; 32] = data[9..41]
        .try_into()
        .map_err(|_| error!(TaxError::TaxOverflow))?;
    Ok(Pubkey::new_from_array(mint_a_bytes))
}

/// Read pool reserves from a PoolState AccountInfo, returning (sol_reserve, token_reserve).
///
/// # Security checks (DEF-01, DEF-02)
/// 1. **Owner verification (DEF-01):** Rejects accounts not owned by AMM program.
///    Prevents spoofed pool accounts from feeding arbitrary reserve data to
///    Tax Program swap calculations and slippage floor enforcement.
/// 2. **is_reversed detection (DEF-02):** Reads mint_a from bytes [9..41] and
///    compares to NATIVE_MINT to determine canonical ordering. Returns reserves
///    in (SOL, token) order regardless of how the AMM stores them.
///
/// # Arguments
/// * `pool_info` - The AMM pool AccountInfo (must be at least 153 bytes)
///
/// # Returns
/// * `Ok((sol_reserve, token_reserve))` - Pool reserves normalized to (SOL, token)
/// * `Err(TaxError::InvalidPoolOwner)` - If account is not owned by AMM program
/// * `Err(TaxError::InvalidPoolType)` - If data is too short
/// * `Err(TaxError::TaxOverflow)` - If byte slice conversion fails
///
/// # Why raw bytes instead of PoolState deserialization
/// The Tax Program has no Cargo dependency on the AMM crate and does not
/// import PoolState. Raw byte reads at known offsets avoid cross-crate
/// coupling. This pattern is proven in Carnage code.
pub fn read_pool_reserves(pool_info: &AccountInfo) -> Result<(u64, u64)> {
    // DEF-01: Verify the pool account is owned by the AMM program.
    // Without this check, an attacker could pass a fake account with
    // arbitrary reserve values, manipulating slippage floor calculations.
    require!(
        *pool_info.owner == amm_program_id(),
        TaxError::InvalidPoolOwner
    );

    let data = pool_info.data.borrow();

    // PoolState minimum size: 8 (discriminator) + 1 (pool_type) + 32*4 (mints+vaults)
    // + 8*2 (reserves) = 153 bytes
    require!(data.len() >= 153, TaxError::InvalidPoolType);

    // DEF-02: Read mint_a to determine canonical ordering.
    // AMM stores pools with mints in canonical (sorted) order. For SOL pools,
    // NATIVE_MINT (0x06...) is always mint_a because it sorts before all token
    // mints. But for safety (and future-proofing), we detect explicitly.
    let mint_a = Pubkey::try_from(&data[9..41]).map_err(|_| error!(TaxError::TaxOverflow))?;

    let reserve_a = u64::from_le_bytes(
        data[137..145]
            .try_into()
            .map_err(|_| error!(TaxError::TaxOverflow))?,
    );
    let reserve_b = u64::from_le_bytes(
        data[145..153]
            .try_into()
            .map_err(|_| error!(TaxError::TaxOverflow))?,
    );

    // If mint_a == NATIVE_MINT: reserve_a is SOL, reserve_b is token (normal order).
    // If mint_a != NATIVE_MINT: pool is reversed, reserve_b is SOL, reserve_a is token.
    if mint_a == native_mint() {
        Ok((reserve_a, reserve_b))
    } else {
        Ok((reserve_b, reserve_a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_anchor_error_code(error: anchor_lang::error::Error, expected: u32) {
        match error {
            anchor_lang::error::Error::AnchorError(anchor_error) => {
                assert_eq!(anchor_error.error_code_number, expected);
            }
            other => panic!("expected Anchor error code {expected}, got {other:?}"),
        }
    }

    #[test]
    fn read_pool_mint_a_returns_planted_mint() {
        let key = Pubkey::new_unique();
        let owner = amm_program_id();
        let planted_mint = Pubkey::new_unique();
        let mut lamports = 0;
        let mut data = vec![0u8; 41];
        data[9..41].copy_from_slice(planted_mint.as_ref());

        let pool_account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_eq!(read_pool_mint_a(&pool_account).unwrap(), planted_mint);
    }

    #[test]
    fn read_pool_mint_a_rejects_wrong_owner() {
        let key = Pubkey::new_unique();
        let wrong_owner = Pubkey::new_unique();
        let mut lamports = 0;
        let mut data = vec![0u8; 41];
        let pool_account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &wrong_owner,
            false,
            0,
        );

        let error = read_pool_mint_a(&pool_account).unwrap_err();
        assert_anchor_error_code(error, 6018);
    }

    #[test]
    fn read_pool_mint_a_rejects_short_data() {
        let key = Pubkey::new_unique();
        let owner = amm_program_id();
        let mut lamports = 0;
        let mut data = vec![0u8; 40];
        let pool_account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        let error = read_pool_mint_a(&pool_account).unwrap_err();
        assert_anchor_error_code(error, 6000);
    }
}
