//! Minimal LiteSVM test-support primitives for the Phase 158 arb suite.
//!
//! ## Scope (Phase 158 — locked decision 2 / Open Question 1)
//!
//! This is the SLIM 158-scoped subset of the heavier `MILF-building`
//! `arb-test-support` crate (a 2037-line Phase-164 consolidation that depends on the
//! Phase-159 deployable arb crate and the arb firing crate — both absent in 158 —
//! plus bincode and the Pyth/transfer-hook surface). Per the minimal-subset decision
//! we port ONLY the helpers the 158 LiteSVM harnesses (the `arb_ix.rs` pattern)
//! import, so the crate compiles standalone with NO dependency on those absent
//! arb crates:
//!
//!   * the Anchor `Pubkey` ↔ litesvm `solana_address::Address` byte-bridge
//!     (`addr` / `pk` / `kp_pubkey`),
//!   * the Anchor discriminator helpers (`anchor_ix_disc` / `anchor_account_disc` /
//!     `anchor_event_disc` = `sha256("global:"/"account:"/"event:" + name)[..8]`),
//!   * the upgradeable-BPF installer (`deploy_upgradeable_program` — how the
//!     mock-arb-caller deploys at the arb program id), with the `.so` path/read
//!     helpers and `program_data_address`,
//!   * the Token-2022 / SPL mint + token-account installers
//!     (`install_t22_mint` / `install_native_wsol_mint` / `install_spl_mint` /
//!     `install_token_account_funded` / `install_placeholder_token_account`),
//!   * the real Token-2022 transfer-hook fixture (`install_hooked_t22_mint` /
//!     `install_hooked_t22_token_account` / `install_extra_account_meta_list` /
//!     `whitelist_token_account` / `install_full_hook_fixture`),
//!   * the single-IX transaction sender (`send_tx`).
//!
//! Every function body below is a FAITHFUL port of the proven MILF-building helper
//! (the byte-identical infrastructure the harnesses already ran); only the stale
//! Phase-164 framing was re-authored to the 158 minimal-subset framing — NO logic
//! was rewritten. NOT a deployable Solana program (plain `lib`); it is linked into
//! the arb test crates as a `[dev-dependencies]`. Plan 06 may ADD to this surface
//! (e.g. PDA derivations / account-meta builders specific to the new lane harness).

#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::result_large_err)]

use std::str::FromStr;

use litesvm::LiteSVM;
use solana_account::Account;
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair as LiteKeypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer as LiteSigner;
use solana_transaction::versioned::VersionedTransaction;

use anchor_lang::prelude::Pubkey;
use sha2::{Digest, Sha256};
use solana_sdk::program_pack::Pack;
use spl_token_2022::state::Account as T22Acct;
use spl_token_2022::state::Mint as T22Mint;

// ===========================================================================
// Common test constants (faithful port — the arb-harness fixture defaults).
// ===========================================================================

/// Faction-mint decimals used by the arb harness fixtures.
pub const TEST_DECIMALS: u8 = 6;
/// AMM LP fee (bps) used at pool init by the arb harness fixtures.
pub const LP_FEE_BPS: u16 = 100;

// ===========================================================================
// Program-id helpers (faithful port — used by the installers below).
// ===========================================================================

pub fn token_2022_program_id() -> Pubkey {
    spl_token_2022::id()
}
pub fn spl_token_program_id() -> Pubkey {
    spl_token::id()
}
pub fn system_program_id() -> Pubkey {
    solana_sdk::system_program::id()
}
pub fn bpf_loader_upgradeable_id() -> Pubkey {
    solana_sdk::bpf_loader_upgradeable::id()
}
pub fn native_mint_id() -> Pubkey {
    spl_token::native_mint::id()
}
/// The SPL Associated Token Account program id. Hardcoded rather than pulled from a
/// crate so the harnesses take no extra dependency for a single well-known constant.
pub fn ata_program_id() -> Pubkey {
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
        .parse()
        .expect("valid ATA program id")
}

/// Canonical associated-token address: `PDA([wallet, token_program, mint], ATA_PROGRAM)`.
///
/// ⚑ 163.1-05: the arb's three working accounts (`caller_crime`/`caller_fraud`/
/// `caller_wsol`) are now ADDRESS-PINNED to exactly this derivation on both cranks
/// (cluster 2 / SOS H019+H017), so every LiteSVM harness that fires the arb MUST install
/// them here — a random keypair address now reverts at account validation with
/// 6016/6017/6018. This is the same derivation the keeper already uses
/// (`getAssociatedTokenAddressSync(mint, arbAuthority, true, tokenProgram)`).
pub fn ata(wallet: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program_id(),
    )
    .0
}

// ===========================================================================
// Address bridge (anchor `Pubkey` ↔ litesvm `solana_address::Address`).
//
// Both are [u8; 32] wrappers; we convert via bytes at the litesvm boundary.
// Faithful port of the harness byte-bridge.
// ===========================================================================

pub fn addr(pk: &Pubkey) -> Address {
    Address::from(pk.to_bytes())
}
pub fn pk(a: &Address) -> Pubkey {
    Pubkey::new_from_array(a.to_bytes())
}
pub fn kp_pubkey(kp: &LiteKeypair) -> Pubkey {
    pk(&kp.pubkey())
}

// ===========================================================================
// Anchor discriminators (faithful port).
// ===========================================================================

/// 8-byte instruction discriminator = sha256("global:<name>")[..8].
pub fn anchor_ix_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{}", name).as_bytes());
    let r = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&r[..8]);
    d
}
/// 8-byte account discriminator = sha256("account:<StructName>")[..8].
pub fn anchor_account_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("account:{}", name).as_bytes());
    let r = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&r[..8]);
    d
}
/// 8-byte EVENT discriminator = sha256("event:<StructName>")[..8].
pub fn anchor_event_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("event:{}", name).as_bytes());
    let r = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&r[..8]);
    d
}

// ===========================================================================
// Upgradeable-program data address (faithful port).
// ===========================================================================

pub fn program_data_address(pid: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[pid.as_ref()], &bpf_loader_upgradeable_id())
}

// ===========================================================================
// .so loading + upgradeable-BPF deploy (faithful port).
//
// This is how the mock-arb-caller deploys at the arb program id (the gate target):
//   deploy_upgradeable_program(&mut svm, &arb_id, &ua, &read_so("mock_arb_caller"))
// ===========================================================================

/// Resolve `target/deploy/{name}.so` (the repo-root deploy dir). `CARGO_MANIFEST_DIR`
/// here is THIS crate (`crates/arb-test-support`); `../../target/deploy` from it ==
/// the repo-root `target/deploy`. Every test crate (`programs/<x>` / `crates/<x>`) is
/// also exactly two levels under the repo root, so the path resolves identically from
/// either a per-harness call site or here.
pub fn target_deploy_path(name: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/../../target/deploy/{}.so", manifest, name)
}

/// Read program bytes from `target/deploy/<name>.so`. Panics with a build hint if
/// missing.
pub fn read_so(name: &str) -> Vec<u8> {
    let path = target_deploy_path(name);
    if !std::path::Path::new(&path).exists() {
        panic!(
            "missing {}.so — build it first (cargo build-sbf --manifest-path programs/{}/Cargo.toml)",
            name, name
        );
    }
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path, e))
}

/// Install an upgradeable BPF program (Program + ProgramData accounts) at
/// `program_id` with `upgrade_authority`. The canonical impl behind
/// [`deploy_upgradeable_program`].
pub fn install_upgradeable_bpf(
    svm: &mut LiteSVM,
    program_id: &Pubkey,
    upgrade_authority: &Pubkey,
    program_bytes: &[u8],
) {
    let (programdata_key, _bump) = program_data_address(program_id);
    let loader_id = bpf_loader_upgradeable_id();

    // UpgradeableLoaderState::ProgramData { slot, upgrade_authority } (variant 3) + ELF.
    let mut pd = Vec::new();
    pd.extend_from_slice(&3u32.to_le_bytes());
    pd.extend_from_slice(&0u64.to_le_bytes());
    pd.push(1); // Some(upgrade_authority)
    pd.extend_from_slice(upgrade_authority.as_ref());
    pd.extend_from_slice(program_bytes);

    let rent = solana_sdk::rent::Rent::default();

    // ProgramData FIRST (litesvm loads the ELF when the executable program account is
    // set and looks up programdata at that moment).
    svm.set_account(
        addr(&programdata_key),
        Account {
            lamports: rent.minimum_balance(pd.len()),
            data: pd,
            owner: addr(&loader_id),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // UpgradeableLoaderState::Program { programdata_address } (variant 2).
    let mut prog = Vec::new();
    prog.extend_from_slice(&2u32.to_le_bytes());
    prog.extend_from_slice(programdata_key.as_ref());
    svm.set_account(
        addr(program_id),
        Account {
            lamports: rent.minimum_balance(prog.len()),
            data: prog,
            owner: addr(&loader_id),
            executable: true,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

/// Install an upgradeable BPF program at `program_id`. Thin wrapper over
/// [`install_upgradeable_bpf`] matching the `arb_ix.rs` harness call shape.
pub fn deploy_upgradeable_program(
    svm: &mut LiteSVM,
    program_id: &Pubkey,
    upgrade_authority: &Pubkey,
    program_bytes: &[u8],
) {
    install_upgradeable_bpf(svm, program_id, upgrade_authority, program_bytes);
}

// ===========================================================================
// Mint installers (faithful port).
// ===========================================================================

pub fn install_t22_mint(svm: &mut LiteSVM, mint: &Pubkey, authority: &Pubkey, decimals: u8) {
    let mut data = vec![0u8; T22Mint::LEN];
    let m = T22Mint {
        mint_authority: solana_sdk::program_option::COption::Some(
            solana_sdk::pubkey::Pubkey::new_from_array(authority.to_bytes()),
        ),
        supply: 0,
        decimals,
        is_initialized: true,
        freeze_authority: solana_sdk::program_option::COption::None,
    };
    T22Mint::pack(m, &mut data).unwrap();
    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(mint),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(&token_2022_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

/// Install a fully serialized Token-2022 mint carrying both transfer-fee
/// schedules. `current_*` is stored in the older/current tuple and
/// `scheduled_*` in a future newer tuple so admission tests can distinguish
/// the two independently.
#[allow(clippy::too_many_arguments)]
pub fn install_t22_transfer_fee_mint(
    svm: &mut LiteSVM,
    mint: &Pubkey,
    authority: &Pubkey,
    decimals: u8,
    current_bps: u16,
    current_maximum: u64,
    scheduled_bps: u16,
    scheduled_maximum: u64,
) {
    use spl_token_2022::extension::{
        transfer_fee::{TransferFee, TransferFeeConfig},
        BaseStateWithExtensionsMut, ExtensionType, StateWithExtensionsMut,
    };

    let space =
        ExtensionType::try_calculate_account_len::<T22Mint>(&[ExtensionType::TransferFeeConfig])
            .expect("calculate transfer-fee mint size");
    let mut data = vec![0u8; space];
    {
        let mut state = StateWithExtensionsMut::<T22Mint>::unpack_uninitialized(&mut data)
            .expect("unpack transfer-fee mint buffer");
        let config = state
            .init_extension::<TransferFeeConfig>(true)
            .expect("initialize TransferFeeConfig fixture");
        config.older_transfer_fee = TransferFee {
            epoch: 0u64.into(),
            maximum_fee: current_maximum.into(),
            transfer_fee_basis_points: current_bps.into(),
        };
        config.newer_transfer_fee = TransferFee {
            epoch: u64::MAX.into(),
            maximum_fee: scheduled_maximum.into(),
            transfer_fee_basis_points: scheduled_bps.into(),
        };
        state.base = T22Mint {
            mint_authority: solana_sdk::program_option::COption::Some(
                solana_sdk::pubkey::Pubkey::new_from_array(authority.to_bytes()),
            ),
            supply: 0,
            decimals,
            is_initialized: true,
            freeze_authority: solana_sdk::program_option::COption::None,
        };
        state.pack_base();
        state
            .init_account_type()
            .expect("initialize transfer-fee mint account type");
    }

    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(mint),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(&token_2022_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install transfer-fee mint fixture");
}

/// Install a Token-2022 token account with the companion TransferFeeAmount
/// extension required by a TransferFeeConfig mint. This lets entrypoint tests
/// prove that a zero/zero fee configuration remains operational, rather than
/// merely proving that the policy parser accepts the mint.
pub fn install_t22_transfer_fee_token_account(
    svm: &mut LiteSVM,
    account: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
) {
    use spl_token_2022::extension::{
        transfer_fee::TransferFeeAmount, BaseStateWithExtensionsMut, ExtensionType,
        StateWithExtensionsMut,
    };

    let space =
        ExtensionType::try_calculate_account_len::<T22Acct>(&[ExtensionType::TransferFeeAmount])
            .expect("calculate transfer-fee token-account size");
    let mut data = vec![0u8; space];
    {
        let mut state = StateWithExtensionsMut::<T22Acct>::unpack_uninitialized(&mut data)
            .expect("unpack transfer-fee token-account buffer");
        state
            .init_extension::<TransferFeeAmount>(true)
            .expect("initialize TransferFeeAmount fixture");
        state.base = T22Acct {
            mint: solana_sdk::pubkey::Pubkey::new_from_array(mint.to_bytes()),
            owner: solana_sdk::pubkey::Pubkey::new_from_array(owner.to_bytes()),
            amount,
            delegate: solana_sdk::program_option::COption::None,
            state: spl_token_2022::state::AccountState::Initialized,
            is_native: solana_sdk::program_option::COption::None,
            delegated_amount: 0,
            close_authority: solana_sdk::program_option::COption::None,
        };
        state.pack_base();
        state
            .init_account_type()
            .expect("initialize transfer-fee token-account type");
    }

    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(account),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(&token_2022_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install transfer-fee token-account fixture");
}

// ===========================================================================
// Real Token-2022 transfer-hook fixture (Phase 169-05; shared with Phase 168).
// ===========================================================================

/// Accounts installed by [`install_full_hook_fixture`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullHookFixture {
    pub extra_account_meta_list: Pubkey,
    pub whitelist_entries: Vec<Pubkey>,
}

/// Create and initialize a Token-2022 mint with its TransferHook extension
/// pointing at `hook_program_id`.
///
/// This drives Token-2022's real `InitializeTransferHook` (instruction 36/0)
/// before `InitializeMint2`; extensions cannot be initialized after the mint.
pub fn install_hooked_t22_mint(
    svm: &mut LiteSVM,
    mint: &LiteKeypair,
    decimals: u8,
    hook_program_id: &Pubkey,
    mint_authority: &LiteKeypair,
) {
    use solana_instruction::account_meta::AccountMeta;
    use spl_token_2022::extension::ExtensionType;

    let mint_key = kp_pubkey(mint);
    let space = ExtensionType::try_calculate_account_len::<T22Mint>(&[ExtensionType::TransferHook])
        .expect("calculate hooked T22 mint size");
    let lamports = solana_sdk::rent::Rent::default().minimum_balance(space);

    let create_account = Instruction {
        program_id: addr(&system_program_id()),
        accounts: vec![
            AccountMeta::new(mint_authority.pubkey(), true),
            AccountMeta::new(mint.pubkey(), true),
        ],
        data: {
            let mut data = vec![0u8; 4 + 8 + 8 + 32];
            data[4..12].copy_from_slice(&lamports.to_le_bytes());
            data[12..20].copy_from_slice(&(space as u64).to_le_bytes());
            data[20..52].copy_from_slice(token_2022_program_id().as_ref());
            data
        },
    };
    let initialize_hook = Instruction {
        program_id: addr(&token_2022_program_id()),
        accounts: vec![AccountMeta::new(mint.pubkey(), false)],
        data: {
            let mut data = vec![36u8, 0u8];
            data.extend_from_slice(kp_pubkey(mint_authority).as_ref());
            data.extend_from_slice(hook_program_id.as_ref());
            data
        },
    };
    let initialize_mint = Instruction {
        program_id: addr(&token_2022_program_id()),
        accounts: vec![
            AccountMeta::new(mint.pubkey(), false),
            AccountMeta::new_readonly(addr(&solana_sdk::sysvar::rent::id()), false),
        ],
        data: {
            let mut data = vec![20u8, decimals];
            data.extend_from_slice(kp_pubkey(mint_authority).as_ref());
            data.push(0); // no freeze authority
            data
        },
    };

    let message = Message::new_with_blockhash(
        &[create_account, initialize_hook, initialize_mint],
        Some(&mint_authority.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[mint_authority, mint])
            .expect("sign hooked-mint transaction");
    svm.send_transaction(transaction)
        .expect("initialize hooked T22 mint");
    svm.expire_blockhash();

    let installed = svm
        .get_account(&addr(&mint_key))
        .expect("hooked T22 mint account exists");
    assert_eq!(installed.owner, addr(&token_2022_program_id()));
}

/// Create a Token-2022 token account with the TransferHookAccount extension.
/// Token-2022 toggles this account's `transferring` flag only while it invokes
/// the configured hook, which makes a successful fixture transfer a real
/// `check_is_transferring` assurance rather than a direct-call simulation.
pub fn install_hooked_t22_token_account(
    svm: &mut LiteSVM,
    payer: &LiteKeypair,
    account: &LiteKeypair,
    mint: &Pubkey,
    owner: &Pubkey,
) {
    use solana_instruction::account_meta::AccountMeta;
    use spl_token_2022::extension::ExtensionType;

    let space =
        ExtensionType::try_calculate_account_len::<T22Acct>(&[ExtensionType::TransferHookAccount])
            .expect("calculate hooked T22 token-account size");
    let lamports = solana_sdk::rent::Rent::default().minimum_balance(space);
    let create_account = Instruction {
        program_id: addr(&system_program_id()),
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(account.pubkey(), true),
        ],
        data: {
            let mut data = vec![0u8; 4 + 8 + 8 + 32];
            data[4..12].copy_from_slice(&lamports.to_le_bytes());
            data[12..20].copy_from_slice(&(space as u64).to_le_bytes());
            data[20..52].copy_from_slice(token_2022_program_id().as_ref());
            data
        },
    };
    let initialize_account = Instruction {
        program_id: addr(&token_2022_program_id()),
        accounts: vec![
            AccountMeta::new(account.pubkey(), false),
            AccountMeta::new_readonly(addr(mint), false),
        ],
        data: {
            let mut data = vec![18u8];
            data.extend_from_slice(owner.as_ref());
            data
        },
    };
    let message = Message::new_with_blockhash(
        &[create_account, initialize_account],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer, account])
            .expect("sign hooked token-account transaction");
    svm.send_transaction(transaction)
        .expect("initialize hooked T22 token account");
    svm.expire_blockhash();
}

/// Mint Token-2022 units into a fixture token account.
pub fn mint_t22_tokens(
    svm: &mut LiteSVM,
    payer: &LiteKeypair,
    mint_authority: &LiteKeypair,
    mint: &Pubkey,
    destination: &Pubkey,
    amount: u64,
) {
    use solana_instruction::account_meta::AccountMeta;

    let instruction = Instruction {
        program_id: addr(&token_2022_program_id()),
        accounts: vec![
            AccountMeta::new(addr(mint), false),
            AccountMeta::new(addr(destination), false),
            AccountMeta::new_readonly(mint_authority.pubkey(), true),
        ],
        data: {
            let mut data = vec![7u8];
            data.extend_from_slice(&amount.to_le_bytes());
            data
        },
    };
    let signers: &[&LiteKeypair] = if payer.pubkey() == mint_authority.pubkey() {
        &[payer]
    } else {
        &[payer, mint_authority]
    };
    send_tx(svm, payer, signers, instruction).expect("mint hooked T22 fixture tokens");
}

/// Install the transfer-hook program's EAML at
/// `PDA([b"extra-account-metas", mint], hook_program_id)`.
///
/// The account is synthesized from the pinned SPL TLV contract: an 8-byte
/// Execute discriminator, a 74-byte pod-slice value (u32 count + two 35-byte
/// ExtraAccountMeta records), and dynamic whitelist PDAs resolved from source
/// account index 0 and destination account index 2. This mirrors
/// `initialize_extra_account_meta_list.rs` without adding another dependency to
/// this intentionally slim shared crate.
pub fn install_extra_account_meta_list(
    svm: &mut LiteSVM,
    hook_program_id: &Pubkey,
    mint: &Pubkey,
) -> Pubkey {
    const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];
    const META_LEN: usize = 35;
    const META_COUNT: usize = 2;
    const VALUE_LEN: usize = 4 + META_COUNT * META_LEN;
    const EAML_LEN: usize = 8 + 4 + VALUE_LEN;

    fn whitelist_meta(account_index: u8) -> [u8; META_LEN] {
        let mut meta = [0u8; META_LEN];
        meta[0] = 1; // PDA owned by the executing hook program
        meta[1] = 1; // Seed::Literal
        meta[2] = 9; // b"whitelist".len()
        meta[3..12].copy_from_slice(b"whitelist");
        meta[12] = 3; // Seed::AccountKey
        meta[13] = account_index;
        // meta[33] is_signer = false; meta[34] is_writable = false.
        meta
    }

    let (address, _) =
        Pubkey::find_program_address(&[b"extra-account-metas", mint.as_ref()], hook_program_id);
    let mut data = vec![0u8; EAML_LEN];
    data[0..8].copy_from_slice(&EXECUTE_DISCRIMINATOR);
    data[8..12].copy_from_slice(&(VALUE_LEN as u32).to_le_bytes());
    data[12..16].copy_from_slice(&(META_COUNT as u32).to_le_bytes());
    data[16..51].copy_from_slice(&whitelist_meta(0));
    data[51..86].copy_from_slice(&whitelist_meta(2));

    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(&address),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(hook_program_id),
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install extra-account-metas fixture");
    address
}

/// Whitelist one TOKEN ACCOUNT for the real transfer hook.
///
/// The hook passes when SOURCE or DESTINATION is whitelisted. Adds therefore
/// whitelist the pool vault destination; removes whitelist the pool vault
/// source. Wallet and payout accounts need no new entries: whitelist pool
/// vaults, not wallets/destinations.
pub fn whitelist_token_account(
    svm: &mut LiteSVM,
    hook_program_id: &Pubkey,
    token_account: &Pubkey,
) -> Pubkey {
    let (address, _) =
        Pubkey::find_program_address(&[b"whitelist", token_account.as_ref()], hook_program_id);
    let mut data = vec![0u8; 48];
    data[0..8].copy_from_slice(&anchor_account_disc("WhitelistEntry"));
    data[8..40].copy_from_slice(token_account.as_ref());
    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(&address),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(hook_program_id),
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install whitelist-entry fixture");
    address
}

/// Install the REAL transfer-hook `.so`, synthesize the mint EAML, and add
/// whitelist entries for the supplied pool token accounts.
pub fn install_full_hook_fixture(
    svm: &mut LiteSVM,
    hook_program_id: &Pubkey,
    upgrade_authority: &Pubkey,
    mint: &Pubkey,
    whitelisted_token_accounts: &[Pubkey],
) -> FullHookFixture {
    install_upgradeable_bpf(
        svm,
        hook_program_id,
        upgrade_authority,
        &read_so("transfer_hook"),
    );
    let extra_account_meta_list = install_extra_account_meta_list(svm, hook_program_id, mint);
    let whitelist_entries = whitelisted_token_accounts
        .iter()
        .map(|account| whitelist_token_account(svm, hook_program_id, account))
        .collect();
    FullHookFixture {
        extra_account_meta_list,
        whitelist_entries,
    }
}

/// Resolve the four remaining accounts expected by Token-2022 for a transfer
/// through this whitelist hook: EAML, source whitelist PDA, destination
/// whitelist PDA, then the hook program.
pub fn full_hook_accounts(
    hook_program_id: &Pubkey,
    mint: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
) -> [Pubkey; 4] {
    [
        Pubkey::find_program_address(&[b"extra-account-metas", mint.as_ref()], hook_program_id).0,
        Pubkey::find_program_address(&[b"whitelist", source.as_ref()], hook_program_id).0,
        Pubkey::find_program_address(&[b"whitelist", destination.as_ref()], hook_program_id).0,
        *hook_program_id,
    ]
}

pub fn install_native_wsol_mint(svm: &mut LiteSVM) {
    let mut data = vec![0u8; spl_token::state::Mint::LEN];
    let m = spl_token::state::Mint {
        mint_authority: solana_sdk::program_option::COption::None,
        supply: 0,
        decimals: 9,
        is_initialized: true,
        freeze_authority: solana_sdk::program_option::COption::None,
    };
    spl_token::state::Mint::pack(m, &mut data).unwrap();
    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(&native_mint_id()),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(&spl_token_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

pub fn install_spl_mint(svm: &mut LiteSVM, mint: &Pubkey, decimals: u8) {
    let mut data = vec![0u8; spl_token::state::Mint::LEN];
    let m = spl_token::state::Mint {
        mint_authority: solana_sdk::program_option::COption::None,
        supply: 0,
        decimals,
        is_initialized: true,
        freeze_authority: solana_sdk::program_option::COption::None,
    };
    spl_token::state::Mint::pack(m, &mut data).unwrap();
    let rent = solana_sdk::rent::Rent::default();
    svm.set_account(
        addr(mint),
        Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: addr(&spl_token_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

// ===========================================================================
// Token-account installers (faithful port).
//
// `install_token_account_funded` (7-arg) installs a REAL `amount` (T22 or SPL,
// native or not). `install_placeholder_token_account` (4-arg) installs a
// ZERO-balance Initialized classic-SPL placeholder. They do genuinely different
// things (the MILF-building crate kept both as distinct installers).
// ===========================================================================

/// 7-arg FUNDED installer — installs a token account with a REAL `amount`.
pub fn install_token_account_funded(
    svm: &mut LiteSVM,
    account: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    is_native: bool,
    t22: bool,
) {
    let rent = solana_sdk::rent::Rent::default();
    if t22 {
        let mut data = vec![0u8; T22Acct::LEN];
        let acct = T22Acct {
            mint: solana_sdk::pubkey::Pubkey::new_from_array(mint.to_bytes()),
            owner: solana_sdk::pubkey::Pubkey::new_from_array(owner.to_bytes()),
            amount,
            delegate: solana_sdk::program_option::COption::None,
            state: spl_token_2022::state::AccountState::Initialized,
            is_native: solana_sdk::program_option::COption::None,
            delegated_amount: 0,
            close_authority: solana_sdk::program_option::COption::None,
        };
        T22Acct::pack(acct, &mut data).unwrap();
        let base = rent.minimum_balance(data.len());
        svm.set_account(
            addr(account),
            Account {
                lamports: base,
                data,
                owner: addr(&token_2022_program_id()),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    } else {
        let mut data = vec![0u8; spl_token::state::Account::LEN];
        let native_flag = if is_native {
            solana_sdk::program_option::COption::Some(
                rent.minimum_balance(spl_token::state::Account::LEN),
            )
        } else {
            solana_sdk::program_option::COption::None
        };
        let acct = spl_token::state::Account {
            mint: solana_sdk::pubkey::Pubkey::new_from_array(mint.to_bytes()),
            owner: solana_sdk::pubkey::Pubkey::new_from_array(owner.to_bytes()),
            amount,
            delegate: solana_sdk::program_option::COption::None,
            state: spl_token::state::AccountState::Initialized,
            is_native: native_flag,
            delegated_amount: 0,
            close_authority: solana_sdk::program_option::COption::None,
        };
        spl_token::state::Account::pack(acct, &mut data).unwrap();
        let base = rent.minimum_balance(data.len()) + if is_native { amount } else { 0 };
        svm.set_account(
            addr(account),
            Account {
                lamports: base,
                data,
                owner: addr(&spl_token_program_id()),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }
}

/// 4-arg PLACEHOLDER installer — installs a ZERO-balance, Initialized classic-SPL
/// token account at a FIXED address (`InterfaceAccount<TokenAccount>` accepts it).
/// Layout: [0..32] mint, [32..64] owner, [64..72] amount (0), [108] state (1 =
/// Initialized).
pub fn install_placeholder_token_account(
    svm: &mut LiteSVM,
    key: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
) {
    let mut data = vec![0u8; spl_token::state::Account::LEN];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    // amount = 0 ([64..72] already zero); state = Initialized (byte 108 = 1).
    data[108] = 1;
    let rent = solana_sdk::rent::Rent::default();
    let lamports = rent.minimum_balance(data.len());
    svm.set_account(
        addr(key),
        Account {
            lamports,
            data,
            owner: addr(&spl_token_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install token account");
}

// ===========================================================================
// Transaction sender (faithful port).
// ===========================================================================

/// Send a single-IX legacy tx; map litesvm error to a String (and expire the
/// blockhash).
pub fn send_tx(
    svm: &mut LiteSVM,
    payer: &LiteKeypair,
    signers: &[&LiteKeypair],
    ix: Instruction,
) -> std::result::Result<(), String> {
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers)
        .map_err(|e| format!("sign: {:?}", e))?;
    let r = svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e.err));
    svm.expire_blockhash();
    r
}

/// The AddressLookupTable native program id.
pub fn alt_program_id() -> Pubkey {
    Pubkey::from_str("AddressLookupTab1e1111111111111111111111111").unwrap()
}

// ===========================================================================
// Phase 163.1-07 — ADDRESS LOOKUP TABLES (the shape production actually sends)
// ===========================================================================

/// Serialized size of `LookupTableMeta` inside an ALT account, before the raw
/// address list. Mirrors `solana_address_lookup_table_interface::state::
/// LOOKUP_TABLE_META_SIZE`.
pub const LOOKUP_TABLE_META_SIZE: usize = 56;

/// Install a READY-TO-USE Address Lookup Table directly into the SVM.
///
/// ## Why this exists (163.1-07)
///
/// A LEGACY transaction carries every account key inline, so it physically cannot hold
/// more than `PACKET_DATA_SIZE / 32` = **38** static keys. The live Prong-2 fire has
/// carried more than that since 162-10 (`scripts/arb/crank-arb.ts`: *"Large account count
/// (41+) → VersionedTransaction v0 + the protocol ALT"*), so production has always sent
/// **v0 + ALT**. The flash bracket adds two more accounts, which pushes the heaviest
/// harness composition past the legacy ceiling too — so the harness now models the
/// production envelope instead of a shape production never uses.
///
/// The account is written with `set_account` rather than through the ALT program's
/// `CreateLookupTable`/`ExtendLookupTable` instructions: those require a recent slot hash
/// and a two-slot activation delay, which a deterministic harness should not have to
/// simulate.
///
/// `last_extended_slot_start_index` is set to the FULL address count so every entry is
/// active at ANY slot — including slot 0, where the runtime's
/// `current_slot > last_extended_slot` fast path does not hold. (Without that, a table
/// installed in a fresh `LiteSVM` resolves ZERO addresses and every lookup fails with
/// `InvalidLookupIndex`.)
pub fn install_address_lookup_table(
    svm: &mut LiteSVM,
    alt_address: &Pubkey,
    authority: &Pubkey,
    addresses: &[Pubkey],
) {
    assert!(
        addresses.len() <= 255,
        "this installer writes last_extended_slot_start_index as a u8; \
         split the table if you need more than 255 addresses"
    );

    let mut data = vec![0u8; LOOKUP_TABLE_META_SIZE + addresses.len() * 32];
    // bincode(ProgramState::LookupTable(LookupTableMeta)) — fixed-width little-endian:
    //   u32 enum discriminant (1 = LookupTable)
    //   u64 deactivation_slot      (u64::MAX = never deactivated)
    //   u64 last_extended_slot     (0)
    //   u8  last_extended_slot_start_index
    //   Option<Pubkey> authority   (1-byte tag + 32)
    //   u16 _padding
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    data[12..20].copy_from_slice(&0u64.to_le_bytes());
    data[20] = addresses.len() as u8;
    data[21] = 1;
    data[22..54].copy_from_slice(authority.as_ref());
    // 54..56 stays zero (_padding).
    for (i, a) in addresses.iter().enumerate() {
        let off = LOOKUP_TABLE_META_SIZE + i * 32;
        data[off..off + 32].copy_from_slice(a.as_ref());
    }

    let lamports = solana_sdk::rent::Rent::default().minimum_balance(data.len());
    svm.set_account(
        addr(alt_address),
        Account {
            lamports,
            data,
            owner: addr(&alt_program_id()),
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install address lookup table");
}

/// Compile a **v0** message that resolves `alt_addresses` through `alt_address`.
///
/// Any account in `alt_addresses` that is not a signer and not an instruction's program
/// id moves out of the static key list and into the table lookup — which is what buys the
/// transaction its headroom.
pub fn v0_message_with_alt(
    svm: &LiteSVM,
    payer: &Pubkey,
    ixs: &[Instruction],
    alt_address: &Pubkey,
    alt_addresses: &[Pubkey],
) -> VersionedMessage {
    let table = solana_message::AddressLookupTableAccount {
        key: addr(alt_address),
        addresses: alt_addresses.iter().map(addr).collect(),
    };
    // The blockhash type is taken straight from litesvm rather than named, so this crate
    // needs no `solana-hash` dependency of its own (a second copy resolves against a
    // different `solana-nonce` and breaks the build).
    VersionedMessage::V0(
        solana_message::v0::Message::try_compile(
            &addr(payer),
            ixs,
            &[table],
            svm.latest_blockhash(),
        )
        .expect("compile v0 message with ALT"),
    )
}

/// Total unique accounts a v0 message locks: STATIC keys + every address resolved through
/// its lookup tables. This is the figure that must stay under the runtime's 64-account
/// ceiling — the static count alone understates it.
pub fn v0_total_account_locks(msg: &VersionedMessage) -> usize {
    match msg {
        VersionedMessage::V0(m) => {
            m.account_keys.len()
                + m.address_table_lookups
                    .iter()
                    .map(|l| l.writable_indexes.len() + l.readonly_indexes.len())
                    .sum::<usize>()
        }
        VersionedMessage::Legacy(m) => m.account_keys.len(),
    }
}

/// STATIC keys only — the ones that consume the 1232-byte packet at 32 bytes each.
pub fn v0_static_account_keys(msg: &VersionedMessage) -> usize {
    match msg {
        VersionedMessage::V0(m) => m.account_keys.len(),
        VersionedMessage::Legacy(m) => m.account_keys.len(),
    }
}

// ===========================================================================
// Phase 163.1-07 — the VAULT FLASH BRACKET composer (shared by every fire harness)
// ===========================================================================

/// The Conversion Vault's flash-loan bracket, as a fire transaction must compose it.
///
/// Since 163.1-06 (cluster 5 / SOS H086) `vault_lend` refuses to move inventory unless
/// a bracket is OPEN, and the bracket is **transaction-scoped**:
///
/// ```text
///   top-level  vault_flash_begin(end_index)   non-CPI; snapshots CRIME/FRAUD/PROFIT,
///                                             sets in_flashloan, and proves through the
///                                             instructions sysvar that a matching
///                                             vault_flash_end sits at `end_index`
///   top-level  <the fire>                     crank / crank_flip — money movement stays
///                                             NESTED (CPI vault_lend -> swaps -> CPI
///                                             vault_replenish); only the OBLIGATION is
///                                             hoisted
///   top-level  vault_flash_end                non-CPI; asserts the faction NOTIONAL SUM
///                                             and PROFIT were restored, clears the flag
/// ```
///
/// Solana commits only if every top-level instruction succeeds ⇒ a committed transaction
/// is one where the closer RAN ⇒ and the closer reverts unless the vault is whole. A
/// transaction that does not commit rolls `vault_lend` back too, so the tokens never
/// leave at all.
///
/// 🚨 **`end_index` is an index into the MESSAGE, not into the fire.** Every leading
/// instruction counts — ComputeBudget, priority fee, ATA create-idempotent — so it can
/// only be computed AFTER the full top-level list is assembled. [`bracket_fire`] is the
/// single place that computation lives; the off-chain mirror is
/// `scripts/arb/lib/arb-fire.ts::bracketFire`.
pub mod flash_bracket {
    use super::*;
    use solana_instruction::account_meta::AccountMeta;

    /// Mirrors `conversion-vault/src/constants.rs`. Pinned by
    /// `bracket_seeds_match_the_vault_program` in the vault's own suite.
    pub const VAULT_CONFIG_SEED: &[u8] = b"vault_config";
    pub const VAULT_CRIME_SEED: &[u8] = b"vault_crime";
    pub const VAULT_FRAUD_SEED: &[u8] = b"vault_fraud";
    pub const VAULT_PROFIT_SEED: &[u8] = b"vault_profit";

    /// The instructions sysvar — one of the only TWO account locks the bracket adds to a
    /// fire transaction (the other is `vault_profit`; `vault_config`, `vault_crime`,
    /// `vault_fraud`, the cranker and the vault program itself are all already in the
    /// fire's account set).
    pub fn instructions_sysvar_id() -> Pubkey {
        Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap()
    }

    /// The four vault accounts BOTH bracket halves take, all pinned by PDA **seeds**.
    ///
    /// Seeds and not `token::authority`: creating a token account owned by an arbitrary
    /// PDA is permissionless, so a mint+authority pin does not identify a unique account
    /// and a caller could hand the opener an empty decoy to snapshot — making the
    /// closer's restoration assert trivially satisfiable.
    #[derive(Clone, Copy, Debug)]
    pub struct BracketAccounts {
        pub vault_config: Pubkey,
        pub vault_crime: Pubkey,
        pub vault_fraud: Pubkey,
        pub vault_profit: Pubkey,
    }

    /// Derive all four from the vault program id.
    pub fn derive(vault_program_id: &Pubkey) -> BracketAccounts {
        let vault_config = Pubkey::find_program_address(&[VAULT_CONFIG_SEED], vault_program_id).0;
        let tok = |seed: &[u8]| {
            Pubkey::find_program_address(&[seed, vault_config.as_ref()], vault_program_id).0
        };
        BracketAccounts {
            vault_config,
            vault_crime: tok(VAULT_CRIME_SEED),
            vault_fraud: tok(VAULT_FRAUD_SEED),
            vault_profit: tok(VAULT_PROFIT_SEED),
        }
    }

    /// `vault_flash_begin(end_index: u8)` instruction data.
    pub fn begin_data(end_index: u8) -> Vec<u8> {
        let mut d = anchor_ix_disc("vault_flash_begin").to_vec();
        d.push(end_index);
        d
    }

    /// `vault_flash_end` instruction data (no args).
    pub fn end_data() -> Vec<u8> {
        anchor_ix_disc("vault_flash_end").to_vec()
    }

    /// Metas after the leading `cranker` signer, shared by both halves.
    fn common_metas(a: &BracketAccounts) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(addr(&a.vault_config), false),
            AccountMeta::new_readonly(addr(&a.vault_crime), false),
            AccountMeta::new_readonly(addr(&a.vault_fraud), false),
            AccountMeta::new_readonly(addr(&a.vault_profit), false),
        ]
    }

    /// The top-level opener.
    pub fn begin_ix(
        vault_program_id: &Pubkey,
        cranker: &Pubkey,
        a: &BracketAccounts,
        end_index: u8,
    ) -> Instruction {
        let mut accounts = vec![AccountMeta::new(addr(cranker), true)];
        accounts.extend(common_metas(a));
        accounts.push(AccountMeta::new_readonly(
            addr(&instructions_sysvar_id()),
            false,
        ));
        Instruction {
            program_id: addr(vault_program_id),
            accounts,
            data: begin_data(end_index),
        }
    }

    /// The top-level closer.
    pub fn end_ix(vault_program_id: &Pubkey, cranker: &Pubkey, a: &BracketAccounts) -> Instruction {
        let mut accounts = vec![AccountMeta::new(addr(cranker), true)];
        accounts.extend(common_metas(a));
        Instruction {
            program_id: addr(vault_program_id),
            accounts,
            data: end_data(),
        }
    }

    /// Compose the bracket around a fire.
    ///
    /// * `leading` — everything that must run BEFORE the bracket opens (ComputeBudget,
    ///   priority fee, ATA create-idempotent). These are real top-level instructions and
    ///   therefore shift `end_index`.
    /// * `inner` — everything that must run INSIDE the bracket (the crank, plus any
    ///   composed siblings such as `dispose_haul`).
    ///
    /// Returns the full top-level list with `end_index` resolved against the assembled
    /// message — the ONE place that arithmetic happens.
    pub fn bracket_fire(
        vault_program_id: &Pubkey,
        cranker: &Pubkey,
        a: &BracketAccounts,
        leading: Vec<Instruction>,
        inner: Vec<Instruction>,
    ) -> Vec<Instruction> {
        let end_index = leading.len() + 1 + inner.len();
        assert!(
            end_index <= u8::MAX as usize,
            "end_index ({}) must fit a u8 — vault_flash_begin takes it as one byte",
            end_index
        );

        let mut ixs = leading;
        ixs.push(begin_ix(vault_program_id, cranker, a, end_index as u8));
        ixs.extend(inner);
        ixs.push(end_ix(vault_program_id, cranker, a));

        assert_eq!(
            ixs.len() - 1,
            end_index,
            "end_index must address the closer we appended"
        );
        ixs
    }

    /// Count the UNIQUE account locks a legacy message takes — the figure that must stay
    /// under the runtime's 64-account ceiling. `Message::new_with_blockhash`
    /// deduplicates, so `account_keys.len()` IS the lock count.
    pub fn unique_lock_count(ixs: &[Instruction], payer: &Pubkey) -> usize {
        Message::new_with_blockhash(ixs, Some(&addr(payer)), &Default::default())
            .account_keys
            .len()
    }
}
