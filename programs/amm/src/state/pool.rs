use anchor_lang::prelude::*;

/// Behavioral pool type, inferred from the token programs of each mint.
///
/// DEVIATION from AMM_Implementation.md Section 4.1:
/// The spec originally defined four protocol-specific variants. Per
/// 09-CONTEXT.md, the AMM is mint-agnostic -- it accepts any mint pair
/// and categorizes by token program combination, not by protocol identity.
///
/// - `MixedPool`: One side uses SPL Token, the other uses Token-2022.
///   Example: CRIME/SOL (T22 + SPL), FRAUD/SOL (T22 + SPL).
/// - `PureT22Pool`: Both sides use Token-2022.
///   Reserved for future use; no active pools use this variant.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum PoolType {
    /// One mint uses SPL Token, the other uses Token-2022.
    MixedPool,
    /// Both mints use Token-2022.
    PureT22Pool,
}

/// On-chain state for a single AMM pool.
///
/// Each pool is a PDA derived from its canonical mint pair:
/// Seeds: [b"pool", mint_a.as_ref(), mint_b.as_ref()]
///
/// Canonical ordering: mint_a < mint_b (byte-wise pubkey comparison).
/// This ensures exactly one pool PDA per unordered mint pair.
///
/// Space: 8 (discriminator) + 1 (pool_type) + 32*2 (mints) + 32*2 (vaults)
///        + 8*2 (reserves) + 2 (fee) + 1 (initialized) + 1 (locked)
///        + 1 (bump) + 1 (vault_a_bump) + 1 (vault_b_bump)
///        + 32*2 (token_programs)
///        = 8 + 216 = 224 bytes total (216 INIT_SPACE).
///
/// DEVIATION from AMM_Implementation.md Section 4.1 (157 bytes):
/// We store vault bumps (2 bytes) and token program keys (64 bytes)
/// on-chain, adding 66 bytes. This avoids re-deriving vault PDAs and
/// re-validating token programs during every swap, reducing compute cost.
#[account]
#[derive(InitSpace)]
pub struct PoolState {
    /// Behavioral pool type (MixedPool or PureT22Pool).
    pub pool_type: PoolType,
    /// First mint in the canonical pair (mint_a < mint_b).
    pub mint_a: Pubkey,
    /// Second mint in the canonical pair.
    pub mint_b: Pubkey,
    /// PDA-owned token account holding reserve A.
    pub vault_a: Pubkey,
    /// PDA-owned token account holding reserve B.
    pub vault_b: Pubkey,
    /// Current reserve of token A (updated on every swap/deposit).
    pub reserve_a: u64,
    /// Current reserve of token B (updated on every swap/deposit).
    pub reserve_b: u64,
    /// LP fee in basis points (e.g., 100 = 1.0%).
    pub lp_fee_bps: u16,
    /// Whether the pool has been fully initialized with liquidity.
    pub initialized: bool,
    /// Reentrancy guard. Set to true during swap execution, cleared after.
    ///
    /// SPEC DEVIATION from AMM_Implementation.md: This field is not in the
    /// original spec. Added for defense-in-depth reentrancy protection per
    /// 11-CONTEXT.md. Solana's runtime borrow rules already prevent same-pool
    /// re-entry via CPI, and CEI ordering handles reserve consistency, but
    /// this provides an explicit belt-and-suspenders guard.
    pub locked: bool,
    /// Pool PDA bump seed.
    pub bump: u8,
    /// Vault A PDA bump seed (avoids re-derivation in swaps).
    pub vault_a_bump: u8,
    /// Vault B PDA bump seed (avoids re-derivation in swaps).
    pub vault_b_bump: u8,
    /// Token program for mint A (SPL Token or Token-2022).
    pub token_program_a: Pubkey,
    /// Token program for mint B (SPL Token or Token-2022).
    pub token_program_b: Pubkey,
}
