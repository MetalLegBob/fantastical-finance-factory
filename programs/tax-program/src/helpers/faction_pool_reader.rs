//! Faction-keyed raw-byte reader for AMM PoolState accounts.
//!
//! New generic quote-asset lanes orient pools by the single CRIME or FRAUD
//! side, never by quote-asset identity. The pure helpers are public so the
//! same shipping logic can be exercised by property and formal proofs.

use amm::state::PoolState;
use anchor_lang::{prelude::*, Discriminator};

use crate::constants::{amm_program_id, crime_mint, fraud_mint};
use crate::errors::TaxError;

const POOL_ACCOUNT_LEN: usize = 224;

/// A validated, faction-oriented view of the AMM pool fields used by Tax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactionPoolView {
    pub faction_is_a: bool,
    pub is_crime: bool,
    pub quote_mint: Pubkey,
    pub faction_reserve: u64,
    pub quote_reserve: u64,
}

/// Classify a pool containing exactly one validated CRIME or FRAUD side.
///
/// Returns `(faction_is_a, is_crime)`. Both canonical faction identities are
/// checked independently; neither-faction and two-faction pool shapes reject.
pub fn classify_faction_pool(
    mint_a: &Pubkey,
    mint_b: &Pubkey,
    crime: &Pubkey,
    fraud: &Pubkey,
) -> Result<(bool, bool)> {
    let a_identity = match (mint_a == crime, mint_a == fraud) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    };
    let b_identity = match (mint_b == crime, mint_b == fraud) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    };

    match (a_identity, b_identity) {
        (Some(is_crime), None) => Ok((true, is_crime)),
        (None, Some(is_crime)) => Ok((false, is_crime)),
        _ => err!(TaxError::NotAFactionPool),
    }
}

/// Map stored A/B reserves to `(faction_reserve, quote_reserve)`.
pub fn oriented_reserves(faction_is_a: bool, reserve_a: u64, reserve_b: u64) -> (u64, u64) {
    if faction_is_a {
        (reserve_a, reserve_b)
    } else {
        (reserve_b, reserve_a)
    }
}

/// Read and validate the faction-oriented fields of an AMM PoolState account.
///
/// PoolState offsets are frozen by `programs/amm/src/state/pool.rs`:
/// mint_a@9..41, mint_b@41..73, reserve_a@137..145,
/// reserve_b@145..153, initialized@155, total account length 224.
pub fn read_faction_pool(pool: &AccountInfo) -> Result<FactionPoolView> {
    require!(*pool.owner == amm_program_id(), TaxError::InvalidPoolOwner);

    let data = pool.data.borrow();
    require!(data.len() == POOL_ACCOUNT_LEN, TaxError::InvalidPoolType);
    require!(
        &data[..8] == PoolState::DISCRIMINATOR,
        TaxError::InvalidPoolType
    );
    require!(data[155] == 1, TaxError::InvalidPoolType);

    let mint_a = Pubkey::try_from(&data[9..41]).map_err(|_| error!(TaxError::TaxOverflow))?;
    let mint_b = Pubkey::try_from(&data[41..73]).map_err(|_| error!(TaxError::TaxOverflow))?;
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

    let (faction_is_a, is_crime) =
        classify_faction_pool(&mint_a, &mint_b, &crime_mint(), &fraud_mint())?;
    let (faction_reserve, quote_reserve) = oriented_reserves(faction_is_a, reserve_a, reserve_b);
    let quote_mint = if faction_is_a { mint_b } else { mint_a };

    Ok(FactionPoolView {
        faction_is_a,
        is_crime,
        quote_mint,
        faction_reserve,
        quote_reserve,
    })
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

    fn pool_data(
        mint_a: Pubkey,
        mint_b: Pubkey,
        reserve_a: u64,
        reserve_b: u64,
        initialized: bool,
    ) -> Vec<u8> {
        let mut data = vec![0u8; POOL_ACCOUNT_LEN];
        data[..8].copy_from_slice(PoolState::DISCRIMINATOR);
        data[9..41].copy_from_slice(mint_a.as_ref());
        data[41..73].copy_from_slice(mint_b.as_ref());
        data[137..145].copy_from_slice(&reserve_a.to_le_bytes());
        data[145..153].copy_from_slice(&reserve_b.to_le_bytes());
        data[155] = u8::from(initialized);
        data
    }

    #[test]
    fn classify_faction_pool_covers_full_truth_table() {
        let crime = Pubkey::new_unique();
        let fraud = Pubkey::new_unique();
        let quote_a = Pubkey::new_unique();
        let quote_b = Pubkey::new_unique();

        assert_eq!(
            classify_faction_pool(&crime, &quote_a, &crime, &fraud).unwrap(),
            (true, true)
        );
        assert_eq!(
            classify_faction_pool(&quote_a, &crime, &crime, &fraud).unwrap(),
            (false, true)
        );
        assert_eq!(
            classify_faction_pool(&fraud, &quote_a, &crime, &fraud).unwrap(),
            (true, false)
        );
        assert_eq!(
            classify_faction_pool(&quote_a, &fraud, &crime, &fraud).unwrap(),
            (false, false)
        );

        let neither = classify_faction_pool(&quote_a, &quote_b, &crime, &fraud)
            .expect_err("a pool with neither faction must reject");
        assert_anchor_error_code(neither, 6022);

        let both = classify_faction_pool(&crime, &fraud, &crime, &fraud)
            .expect_err("a pool with both factions must reject");
        assert_anchor_error_code(both, 6022);
    }

    #[test]
    fn oriented_reserves_is_slot_agnostic() {
        for (reserve_a, reserve_b) in [(0, 0), (17, 93), (u64::MAX, 1)] {
            assert_eq!(
                oriented_reserves(true, reserve_a, reserve_b),
                oriented_reserves(false, reserve_b, reserve_a)
            );
        }
    }

    #[test]
    fn raw_reader_returns_oriented_views_for_both_slots() {
        let key = Pubkey::new_unique();
        let owner = amm_program_id();
        let quote = Pubkey::new_unique();

        let mut a_lamports = 0;
        let mut a_data = pool_data(crime_mint(), quote, 41, 97, true);
        let a_account = AccountInfo::new(
            &key,
            false,
            false,
            &mut a_lamports,
            &mut a_data,
            &owner,
            false,
            0,
        );
        assert_eq!(
            read_faction_pool(&a_account).unwrap(),
            FactionPoolView {
                faction_is_a: true,
                is_crime: true,
                quote_mint: quote,
                faction_reserve: 41,
                quote_reserve: 97,
            }
        );

        let mut b_lamports = 0;
        let mut b_data = pool_data(quote, fraud_mint(), 113, 59, true);
        let b_account = AccountInfo::new(
            &key,
            false,
            false,
            &mut b_lamports,
            &mut b_data,
            &owner,
            false,
            0,
        );
        assert_eq!(
            read_faction_pool(&b_account).unwrap(),
            FactionPoolView {
                faction_is_a: false,
                is_crime: false,
                quote_mint: quote,
                faction_reserve: 59,
                quote_reserve: 113,
            }
        );
    }

    #[test]
    fn raw_reader_rejects_wrong_owner() {
        let key = Pubkey::new_unique();
        let wrong_owner = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let mut lamports = 0;
        let mut data = pool_data(crime_mint(), quote, 1, 2, true);
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &wrong_owner,
            false,
            0,
        );

        assert_anchor_error_code(read_faction_pool(&account).unwrap_err(), 6018);
    }

    #[test]
    fn raw_reader_rejects_non_exact_data_length() {
        let key = Pubkey::new_unique();
        let owner = amm_program_id();
        let mut lamports = 0;
        let mut data = vec![0u8; POOL_ACCOUNT_LEN - 1];
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_anchor_error_code(read_faction_pool(&account).unwrap_err(), 6000);
    }

    #[test]
    fn raw_reader_rejects_wrong_discriminator() {
        let key = Pubkey::new_unique();
        let owner = amm_program_id();
        let quote = Pubkey::new_unique();
        let mut lamports = 0;
        let mut data = pool_data(crime_mint(), quote, 1, 2, true);
        data[0] ^= 0xff;
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_anchor_error_code(read_faction_pool(&account).unwrap_err(), 6000);
    }

    #[test]
    fn raw_reader_rejects_uninitialized_pool() {
        let key = Pubkey::new_unique();
        let owner = amm_program_id();
        let quote = Pubkey::new_unique();
        let mut lamports = 0;
        let mut data = pool_data(crime_mint(), quote, 1, 2, false);
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_anchor_error_code(read_faction_pool(&account).unwrap_err(), 6000);
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    const CASES: u32 = 100_000;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(CASES))]

        #[test]
        fn crime_on_a_classifies_for_every_non_faction_countermint(bytes in any::<[u8; 32]>()) {
            let crime = Pubkey::new_from_array([1; 32]);
            let fraud = Pubkey::new_from_array([2; 32]);
            let mint_b = Pubkey::new_from_array(bytes);
            prop_assume!(mint_b != crime && mint_b != fraud);

            let classified = classify_faction_pool(&crime, &mint_b, &crime, &fraud);
            prop_assert_eq!(classified.unwrap(), (true, true));
        }

        #[test]
        fn every_neither_faction_pair_rejects(
            a_bytes in any::<[u8; 32]>(),
            b_bytes in any::<[u8; 32]>(),
        ) {
            let crime = Pubkey::new_from_array([1; 32]);
            let fraud = Pubkey::new_from_array([2; 32]);
            let mint_a = Pubkey::new_from_array(a_bytes);
            let mint_b = Pubkey::new_from_array(b_bytes);
            prop_assume!(mint_a != crime && mint_a != fraud);
            prop_assume!(mint_b != crime && mint_b != fraud);

            prop_assert!(classify_faction_pool(&mint_a, &mint_b, &crime, &fraud).is_err());
        }

        #[test]
        fn reserve_orientation_is_invariant_under_slot_flip(
            faction_is_a in any::<bool>(),
            reserve_a in any::<u64>(),
            reserve_b in any::<u64>(),
        ) {
            prop_assert_eq!(
                oriented_reserves(faction_is_a, reserve_a, reserve_b),
                oriented_reserves(!faction_is_a, reserve_b, reserve_a)
            );
        }
    }
}
