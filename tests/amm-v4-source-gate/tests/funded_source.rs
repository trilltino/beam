//! Local-only source compatibility gate; always rolls back.
#[cfg(feature = "sbf-raydium")]
#[path = "../../sbf-support/artifact.rs"]
mod sbf_artifact;
use std::sync::atomic::{AtomicUsize, Ordering};

use anchor_lang::solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError, pubkey,
    pubkey::Pubkey, rent::Rent, system_program,
};
use anchor_lang::{AccountSerialize, Discriminator, InstructionData};
use beam_program::{
    beam_amm_v4::{
        derive_pool_addresses, initialize2_instruction, user_lp_ata, Initialize2Accounts,
        Initialize2Args, MintKey, ASSOCIATED_TOKEN_PROGRAM_ID, DEVNET_PROGRAM_ID,
        MAINNET_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    trade_types::BondingCurve,
    Global,
};
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta as SdkMeta, Instruction as SdkInstruction},
    pubkey::Pubkey as SdkPubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

#[cfg(all(feature = "devnet", feature = "mainnet"))]
compile_error!("select exactly one identity profile: devnet or mainnet");
#[cfg(not(any(feature = "devnet", feature = "mainnet")))]
compile_error!("select exactly one identity profile: devnet or mainnet");

const PROFILE: &str = if cfg!(feature = "devnet") {
    "devnet"
} else {
    "mainnet"
};
const AMM_ID: Pubkey = if cfg!(feature = "devnet") {
    DEVNET_PROGRAM_ID
} else {
    MAINNET_PROGRAM_ID
};
const OTHER_AMM_ID: Pubkey = if cfg!(feature = "devnet") {
    MAINNET_PROGRAM_ID
} else {
    DEVNET_PROGRAM_ID
};

const MINT: Pubkey = Pubkey::new_from_array([42; 32]);
const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
const BASE: u64 = 2_000_000;
const QUOTE: u64 = 2_000_000;
const NATIVE_RENT: u64 = 3_000_000;
const MIGRATION_FEE: u64 = 100_000;
const CREATOR_FEE: u64 = 30_000;
const PROTOCOL_FEES: u64 = 20_000;
const DONOR_SURPLUS: u64 = 70_000;
const POST_COMPLETE_QUOTE: u64 = 250_000;
const SETUP_DONATION: u64 = 17_000;
const SETUP_BUDGET: u64 = 70_000_000;
const MIGRATION_RECIPIENT: Pubkey = Pubkey::new_from_array([96; 32]);

static OTHER_PROGRAM_CALLS: AtomicUsize = AtomicUsize::new(0);
static CPI_CALLS: AtomicUsize = AtomicUsize::new(0);
static SOURCE_SUCCESSES: AtomicUsize = AtomicUsize::new(0);
static LATE_SOURCE_FAILURES: AtomicUsize = AtomicUsize::new(0);

// The test bank and Anchor helpers can instantiate different Pubkey crates.
fn sdk(key: Pubkey) -> SdkPubkey {
    SdkPubkey::new_from_array(key.to_bytes())
}
fn anchor(key: SdkPubkey) -> Pubkey {
    Pubkey::new_from_array(key.to_bytes())
}
fn account(owner: Pubkey, lamports: u64, data: Vec<u8>, executable: bool) -> Account {
    Account {
        lamports,
        data,
        owner: sdk(owner),
        executable,
        rent_epoch: 0,
    }
}
fn put_u64(data: &mut [u8], at: usize, value: u64) {
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn put_key(data: &mut [u8], at: usize, key: Pubkey) {
    data[at..at + 32].copy_from_slice(key.as_ref());
}
fn mint_bytes(decimals: u8) -> Vec<u8> {
    let mut data = vec![0; 82];
    data[44] = decimals;
    data[45] = 1;
    data
}
fn token_bytes(mint: Pubkey, owner: Pubkey, amount: u64, native: bool) -> Vec<u8> {
    let mut data = vec![0; 165];
    put_key(&mut data, 0, mint);
    put_key(&mut data, 32, owner);
    put_u64(&mut data, 64, amount);
    data[108] = 1;
    if native {
        data[109..113].copy_from_slice(&1u32.to_le_bytes());
        put_u64(&mut data, 113, NATIVE_RENT);
    }
    data
}
fn curve_bytes_with(
    base_supply: u64,
    real_quote_reserves: u64,
    post_complete_quote_in: u64,
    creator_fee: u64,
    protocol_fees: u64,
) -> Vec<u8> {
    let curve = BondingCurve {
        virtual_token_reserves: 0,
        virtual_quote_reserves: u64::from(real_quote_reserves != 0),
        real_token_reserves: 0,
        real_quote_reserves,
        token_total_supply: base_supply,
        complete: true,
        creator: Pubkey::default(),
        is_mayhem_mode: false,
        is_cashback_coin: false,
        quote_mint: Pubkey::default(),
        creator_fee_bps: 0,
        can_edit_creator_fee: false,
        is_holder_reward: false,
        creator_fee,
        protocol_fees,
        depth: 0,
        initial_virtual_quote_reserves: 0,
        post_complete_base_out: 0,
        post_complete_quote_in,
    };
    let mut data = Vec::new();
    curve.try_serialize(&mut data).unwrap();
    data
}
fn global_bytes(authority: Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; 2048];
    data[..8].copy_from_slice(Global::DISCRIMINATOR);
    data[8] = 1;
    put_key(&mut data, 9, authority);
    data
}
fn funded_global_bytes(authority: Pubkey) -> Vec<u8> {
    let mut data = global_bytes(authority);
    // Current Global layout: discriminator 8, initialized 1, authority 32,
    // fee recipient 32, four u64s, withdrawal authority 32, enable 1.
    put_key(&mut data, 113, MIGRATION_RECIPIENT);
    put_u64(&mut data, 146, MIGRATION_FEE);
    data
}

fn beam_processor<'a>(id: &SdkPubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    // SAFETY: synchronous ProgramTest dispatch; entry does not retain the slice.
    let infos: &'a [AccountInfo<'a>] = unsafe { std::mem::transmute(infos) };
    beam_program::entry(id, infos, data)
}

// An executable opposite-profile identity fixture. It must never execute;
// this trap is not an AMM implementation and loads no captured binary.
fn unexpected_profile<'a>(_: &SdkPubkey, _: &[AccountInfo<'a>], _: &[u8]) -> ProgramResult {
    OTHER_PROGRAM_CALLS.fetch_add(1, Ordering::SeqCst);
    Err(ProgramError::IncorrectProgramId)
}

fn pinned_raydium<'a>(id: &SdkPubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    CPI_CALLS.fetch_add(1, Ordering::SeqCst);
    let fee_before = infos[13].lamports();
    let result = raydium_amm::processor::Processor::process(id, infos, data);
    if result.is_ok() {
        SOURCE_SUCCESSES.fetch_add(1, Ordering::SeqCst);
        assert_eq!(infos[4].owner, id);
        assert_eq!(infos[4].data_len(), 752);
        assert_eq!(infos[11].data_len(), 2208);
        assert_eq!(
            u64::from_le_bytes(infos[9].try_borrow_data()?[64..72].try_into().unwrap()),
            BASE
        );
        assert_eq!(
            u64::from_le_bytes(infos[10].try_borrow_data()?[64..72].try_into().unwrap()),
            QUOTE
        );
        assert_eq!(
            u64::from_le_bytes(infos[18].try_borrow_data()?[64..72].try_into().unwrap()),
            1_000_000
        );
    }
    if result.is_err() && infos[11].owner == id && infos[11].data_len() == 2208 {
        // Insufficient setup rent fails after the real fee transfer and
        // target/mint/vault allocations, before the pool can be allocated.
        assert_eq!(infos[13].lamports(), fee_before + 50_000);
        assert_eq!(infos[6].data_len(), 82);
        assert_eq!(infos[9].data_len(), 165);
        assert_eq!(infos[10].data_len(), 165);
        assert!(infos[4].data_is_empty());
        LATE_SOURCE_FAILURES.fetch_add(1, Ordering::SeqCst);
    }
    result
}
struct Fixture {
    bank: ProgramTestContext,
    admin: Keypair,
    instruction: SdkInstruction,
    pool: SdkPubkey,
    observed_keys: Vec<SdkPubkey>,
}

#[derive(Clone, Copy, Debug, Default)]
enum IdentityMismatch {
    #[default]
    None,
    Program,
    ConfigAddress,
    ConfigOwner,
    FeeDestination,
}

#[derive(Clone, Copy, Default)]
struct FundedCase {
    identity: IdentityMismatch,
    base_source_donation: u64,
    wsol_source_donation: u64,
    curve_base_vault_donation: u64,
    recipient_aliases_curve: bool,
    recipient_aliases_pool: bool,
    insufficient_curve_backing: bool,
    overflowing_quote: bool,
    too_small_liquidity: bool,
    precreated_pool: bool,
    active_mint_authority: bool,
    active_freeze_authority: bool,
}

async fn funded_fixture(case: FundedCase) -> Fixture {
    // Ephemeral local test signer only; never persisted or used with RPC.
    let admin = Keypair::new();
    let base = if case.too_small_liquidity { 1 } else { BASE };
    let (signer, _) =
        Pubkey::find_program_address(&[b"pool-authority", MINT.as_ref()], &beam_program::ID);
    let market =
        Pubkey::find_program_address(&[b"amm-v4-market", MINT.as_ref()], &beam_program::ID).0;
    let pool = derive_pool_addresses(&AMM_ID, &market).unwrap();
    let other = derive_pool_addresses(&OTHER_AMM_ID, &market).unwrap();
    assert_eq!(
        pool.config,
        Pubkey::find_program_address(&[b"amm_config_account_seed"], &raydium_amm::id()).0
    );
    assert_eq!(
        pool.create_pool_fee_destination,
        raydium_amm::processor::config_feature::create_pool_fee_address::id()
    );
    let curve =
        Pubkey::find_program_address(&[b"bonding-curve", MINT.as_ref()], &beam_program::ID).0;
    let global = Pubkey::find_program_address(&[b"global"], &beam_program::ID).0;
    let curve_base_vault = user_lp_ata(&curve, &MINT);
    let sources = Initialize2Accounts {
        coin_mint: MintKey {
            key: MINT,
            owner: SPL_TOKEN_PROGRAM_ID,
        },
        pc_mint: MintKey {
            key: WSOL,
            owner: SPL_TOKEN_PROGRAM_ID,
        },
        market_seed: market,
        user_wallet: signer,
        user_coin_source: user_lp_ata(&signer, &MINT),
        user_pc_source: user_lp_ata(&signer, &WSOL),
        user_lp_ata: user_lp_ata(&signer, &pool.lp_mint),
    };
    let mut inner = initialize2_instruction(
        AMM_ID,
        &sources,
        Initialize2Args {
            open_time: 3,
            init_coin_amount: base,
            init_pc_amount: QUOTE,
        },
    )
    .unwrap();
    if matches!(case.identity, IdentityMismatch::ConfigAddress) {
        inner.accounts[12].pubkey = other.config;
    }
    if matches!(case.identity, IdentityMismatch::FeeDestination) {
        inner.accounts[13].pubkey = other.create_pool_fee_destination;
    }
    let mut bank = ProgramTest::default();
    // Never select an ambient .so via BPF_OUT_DIR/SBF_OUT_DIR.
    bank.prefer_bpf(false);
    bank.add_program(
        "beam_program",
        sdk(beam_program::ID),
        processor!(beam_processor),
    );
    #[cfg(not(feature = "sbf-raydium"))]
    bank.add_program(
        "pinned_raydium_amm_v4",
        sdk(AMM_ID),
        processor!(pinned_raydium),
    );
    #[cfg(feature = "sbf-raydium")]
    bank.add_account(sdk(AMM_ID), sbf_artifact::account("RAYDIUM_SBF"));
    bank.add_program(
        "opposite_profile_must_not_execute",
        sdk(OTHER_AMM_ID),
        processor!(unexpected_profile),
    );
    bank.add_program(
        "spl_token_7",
        sdk(SPL_TOKEN_PROGRAM_ID),
        processor!(spl_token::processor::Processor::process),
    );
    bank.add_program(
        "spl_ata_6",
        sdk(ASSOCIATED_TOKEN_PROGRAM_ID),
        processor!(spl_associated_token_account::processor::process_instruction),
    );
    let real_quote = if case.overflowing_quote {
        u64::MAX
    } else {
        QUOTE + MIGRATION_FEE - POST_COMPLETE_QUOTE
    };
    let post_quote = if case.overflowing_quote {
        1
    } else {
        POST_COMPLETE_QUOTE
    };
    let curve_data = curve_bytes_with(base, real_quote, post_quote, CREATOR_FEE, PROTOCOL_FEES);
    let protected = Rent::default().minimum_balance(curve_data.len()) + CREATOR_FEE + PROTOCOL_FEES;
    let curve_lamports = protected + QUOTE + MIGRATION_FEE + DONOR_SURPLUS
        - if case.insufficient_curve_backing {
            DONOR_SURPLUS + 1
        } else {
            0
        };
    let mut global_data = funded_global_bytes(anchor(admin.pubkey()));
    let recipient = if case.recipient_aliases_curve {
        curve
    } else if case.recipient_aliases_pool {
        pool.amm
    } else {
        MIGRATION_RECIPIENT
    };
    put_key(&mut global_data, 113, recipient);
    bank.add_account(
        sdk(global),
        account(beam_program::ID, 100_000_000, global_data, false),
    );
    bank.add_account(
        sdk(curve),
        account(beam_program::ID, curve_lamports, curve_data, false),
    );
    bank.add_account(
        sdk(curve_base_vault),
        account(
            SPL_TOKEN_PROGRAM_ID,
            100_000_000,
            token_bytes(MINT, curve, base + case.curve_base_vault_donation, false),
            false,
        ),
    );
    bank.add_account(
        sdk(signer),
        account(system_program::ID, SETUP_DONATION, vec![], false),
    );
    if !case.recipient_aliases_curve && !case.recipient_aliases_pool {
        bank.add_account(
            sdk(recipient),
            account(system_program::ID, 17_000, vec![], false),
        );
    }
    let mut base_mint_data = mint_bytes(6);
    put_u64(
        &mut base_mint_data,
        36,
        base + case.curve_base_vault_donation + case.base_source_donation,
    );
    if case.active_mint_authority {
        base_mint_data[..4].copy_from_slice(&1u32.to_le_bytes());
        put_key(&mut base_mint_data, 4, Pubkey::new_from_array([97; 32]));
    }
    if case.active_freeze_authority {
        base_mint_data[46..50].copy_from_slice(&1u32.to_le_bytes());
        put_key(&mut base_mint_data, 50, Pubkey::new_from_array([98; 32]));
    }
    bank.add_account(
        sdk(MINT),
        account(SPL_TOKEN_PROGRAM_ID, 100_000_000, base_mint_data, false),
    );
    bank.add_account(
        sdk(WSOL),
        account(SPL_TOKEN_PROGRAM_ID, 100_000_000, mint_bytes(9), false),
    );
    for (index, meta) in inner.accounts.iter().enumerate() {
        if matches!(index, 0 | 1 | 2 | 3 | 7 | 8 | 15) {
            continue;
        }
        let key = meta.pubkey;
        let (owner, lamports, data, executable) = match index {
            4 | 6 | 9 | 10 | 11 | 18 => (
                system_program::ID,
                u64::from(case.precreated_pool && index == 4),
                vec![],
                false,
            ),
            5 => (system_program::ID, 1, vec![], false),
            12 => {
                let mut d = vec![0; 544];
                put_u64(&mut d, 536, 50_000);
                let owner = if matches!(
                    case.identity,
                    IdentityMismatch::ConfigAddress | IdentityMismatch::ConfigOwner
                ) {
                    OTHER_AMM_ID
                } else {
                    AMM_ID
                };
                (owner, 100_000_000, d, false)
            }
            13 => (
                SPL_TOKEN_PROGRAM_ID,
                NATIVE_RENT,
                token_bytes(WSOL, Pubkey::new_from_array([85; 32]), 0, true),
                false,
            ),
            14 => (system_program::ID, 1, vec![], false),
            16 => (
                SPL_TOKEN_PROGRAM_ID,
                100_000_000,
                token_bytes(MINT, signer, case.base_source_donation, false),
                false,
            ),
            17 => (
                SPL_TOKEN_PROGRAM_ID,
                NATIVE_RENT + case.wsol_source_donation,
                token_bytes(WSOL, signer, case.wsol_source_donation, true),
                false,
            ),
            _ => unreachable!("account index {index}"),
        };
        bank.add_account(sdk(key), account(owner, lamports, data, executable));
    }
    bank.add_account(
        admin.pubkey(),
        account(system_program::ID, 100_000_000, vec![], false),
    );
    let outer_program = if matches!(case.identity, IdentityMismatch::Program) {
        OTHER_AMM_ID
    } else {
        AMM_ID
    };
    let mut metas = vec![
        SdkMeta::new_readonly(sdk(global), false),
        SdkMeta::new(admin.pubkey(), true),
        SdkMeta::new_readonly(sdk(MINT), false),
        SdkMeta::new(sdk(curve), false),
        SdkMeta::new(sdk(curve_base_vault), false),
        SdkMeta::new(sdk(signer), false),
        SdkMeta::new(sdk(sources.user_coin_source), false),
        SdkMeta::new_readonly(sdk(WSOL), false),
        SdkMeta::new(sdk(sources.user_pc_source), false),
        SdkMeta::new(sdk(recipient), false),
        SdkMeta::new_readonly(sdk(outer_program), false),
        SdkMeta::new_readonly(sdk(SPL_TOKEN_PROGRAM_ID), false),
        SdkMeta::new_readonly(sdk(system_program::ID), false),
    ];
    metas.extend(
        inner
            .accounts
            .iter()
            .enumerate()
            .map(|(index, meta)| SdkMeta {
                pubkey: sdk(meta.pubkey),
                is_signer: false,
                is_writable: meta.is_writable || index == 15,
            }),
    );
    let instruction = SdkInstruction {
        program_id: sdk(beam_program::ID),
        accounts: metas,
        data: beam_program::instruction::ProbeAmmV4FundedInitialize2 {
            open_time: 3,
            setup_budget: SETUP_BUDGET,
            setup_rent_cap: 60_000_000,
        }
        .data(),
    };
    // Include every referenced account, plus both canonical profile config/
    // fee destinations, so substitutions cannot hide changes on either side.
    // The distinct transaction fee payer is intentionally excluded.
    let mut observed_keys: Vec<_> = instruction
        .accounts
        .iter()
        .map(|meta| meta.pubkey)
        .collect();
    observed_keys.extend([
        sdk(pool.config),
        sdk(pool.create_pool_fee_destination),
        sdk(other.config),
        sdk(other.create_pool_fee_destination),
        sdk(AMM_ID),
        sdk(OTHER_AMM_ID),
    ]);
    observed_keys.sort_unstable();
    observed_keys.dedup();
    Fixture {
        bank: bank.start_with_context().await,
        admin,
        instruction,
        pool: sdk(pool.amm),
        observed_keys,
    }
}

async fn funded_attempt(f: &mut Fixture, open_time: u64) -> String {
    funded_attempt_with_budget(f, open_time, SETUP_BUDGET, 60_000_000).await
}

async fn funded_attempt_with_budget(
    f: &mut Fixture,
    open_time: u64,
    setup_budget: u64,
    setup_rent_cap: u64,
) -> String {
    f.instruction.data = beam_program::instruction::ProbeAmmV4FundedInitialize2 {
        open_time,
        setup_budget,
        setup_rent_cap,
    }
    .data();
    let tx = Transaction::new_signed_with_payer(
        &[f.instruction.clone()],
        Some(&f.bank.payer.pubkey()),
        &[&f.bank.payer, &f.admin],
        f.bank.last_blockhash,
    );
    let processed = f
        .bank
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    #[cfg(feature = "sbf-raydium")]
    {
        let metadata = processed
            .metadata
            .as_ref()
            .expect("transaction metadata required");
        // ProgramTest's native bridge can repeat adjacent lifecycle events.
        // Normalize only those duplicates before counting inner invocations.
        let mut normalized = Vec::new();
        for log in &metadata.log_messages {
            let lifecycle = log
                .strip_prefix("Program ")
                .and_then(|line| line.split_once(' '))
                .is_some_and(|(id, event)| {
                    id != "log:"
                        && id != "data:"
                        && (event.starts_with("invoke [")
                            || event == "success"
                            || event.starts_with("failed: ")
                            || event.starts_with("consumed "))
                });
            if !lifecycle || normalized.last() != Some(log) {
                normalized.push(log.clone());
            }
        }
        let logs = &normalized;
        for log in logs {
            println!("SBF_TRACE: {log}");
        }
        let entered = format!("Program {AMM_ID} invoke [2]");
        let success = format!("Program {AMM_ID} success");
        let count = logs.iter().filter(|log| **log == entered).count();
        assert!(count <= 1, "unexpected repeated Raydium CPI");
        CPI_CALLS.fetch_add(count, Ordering::SeqCst);
        SOURCE_SUCCESSES.fetch_add(
            logs.iter().filter(|log| **log == success).count(),
            Ordering::SeqCst,
        );
        if let Some(start) = logs.iter().position(|log| *log == entered) {
            let inner = &logs[start..];
            assert!(
                inner
                    .iter()
                    .any(|log| log.starts_with(&format!("Program {AMM_ID} consumed "))),
                "expected SBF compute metering"
            );
            if inner
                .iter()
                .any(|log| *log == format!("Program {AMM_ID} failed: custom program error: 0x1"))
            {
                // Log evidence of the late failing path, not direct observation
                // of intermediate SBF account bytes or fee/reserve deltas.
                assert_eq!(
                    inner
                        .iter()
                        .filter(|log| **log == format!("Program {SPL_TOKEN_PROGRAM_ID} invoke [3]"))
                        .count(),
                    4
                );
                assert_eq!(
                    inner
                        .iter()
                        .filter(|log| **log == format!("Program {SPL_TOKEN_PROGRAM_ID} success"))
                        .count(),
                    4
                );
                assert_eq!(
                    inner
                        .iter()
                        .filter(|log| **log == format!("Program {} invoke [3]", system_program::ID))
                        .count(),
                    14 // fee + four transfer/allocate/assign sequences + failing pool funding.
                );
                assert_eq!(
                    inner
                        .iter()
                        .filter(|log| **log == format!("Program {} success", system_program::ID))
                        .count(),
                    13
                );
                LATE_SOURCE_FAILURES.fetch_add(1, Ordering::SeqCst);
            }
        }
        assert!(metadata.compute_units_consumed > 0);
    }
    format!("{:?}", processed.result.unwrap_err())
}

async fn funded_state(f: &mut Fixture) -> Vec<Option<Account>> {
    let mut state = Vec::new();
    for key in &f.observed_keys {
        state.push(f.bank.banks_client.get_account(*key).await.unwrap());
    }
    state
}

#[tokio::test]
async fn beam_funded_diagnostic_reaches_compatible_pinned_source_and_always_rolls_back() {
    OTHER_PROGRAM_CALLS.store(0, Ordering::SeqCst);
    CPI_CALLS.store(0, Ordering::SeqCst);
    SOURCE_SUCCESSES.store(0, Ordering::SeqCst);
    LATE_SOURCE_FAILURES.store(0, Ordering::SeqCst);
    assert_eq!(raydium_amm::id(), AMM_ID);
    let mut f = funded_fixture(FundedCase::default()).await;
    let before = funded_state(&mut f).await;
    let result = funded_attempt(&mut f, 3).await;
    assert!(
        result.contains("Custom(7002)"),
        "terminal Beam rollback gate: {result}"
    );
    assert_eq!(CPI_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(SOURCE_SUCCESSES.load(Ordering::SeqCst), 1);
    assert_eq!(funded_state(&mut f).await, before);
    assert!(f
        .bank
        .banks_client
        .get_account(f.pool)
        .await
        .unwrap()
        .is_none());
    f.bank.get_new_latest_blockhash().await.unwrap();
    let replay = funded_attempt(&mut f, 3).await;
    assert!(replay.contains("Custom(7002)"), "{replay}");
    assert_eq!(CPI_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(SOURCE_SUCCESSES.load(Ordering::SeqCst), 2);
    assert_eq!(funded_state(&mut f).await, before);

    let mut late = funded_fixture(FundedCase::default()).await;
    let before_late = funded_state(&mut late).await;
    let result = funded_attempt_with_budget(&mut late, 3, 25_000_000, 0).await;
    let code = 1; // System program: insufficient lamports to allocate pool rent.
    assert!(
        result.contains(&format!("Custom({code})")),
        "late source failure: {result}"
    );
    assert_eq!(CPI_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(SOURCE_SUCCESSES.load(Ordering::SeqCst), 2);
    assert_eq!(LATE_SOURCE_FAILURES.load(Ordering::SeqCst), 1);
    assert_eq!(funded_state(&mut late).await, before_late);
    drop(late);

    for (case, code) in [
        #[cfg(feature = "mint-authority-guard")]
        (
            FundedCase {
                active_mint_authority: true,
                ..FundedCase::default()
            },
            7003,
        ),
        #[cfg(feature = "mint-authority-guard")]
        (
            FundedCase {
                active_freeze_authority: true,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                too_small_liquidity: true,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                precreated_pool: true,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                base_source_donation: 1,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                wsol_source_donation: 1,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                recipient_aliases_curve: true,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                recipient_aliases_pool: true,
                ..FundedCase::default()
            },
            7003,
        ),
        (
            FundedCase {
                curve_base_vault_donation: 1,
                ..FundedCase::default()
            },
            7004,
        ),
        (
            FundedCase {
                insufficient_curve_backing: true,
                ..FundedCase::default()
            },
            7004,
        ),
        (
            FundedCase {
                overflowing_quote: true,
                ..FundedCase::default()
            },
            7004,
        ),
    ] {
        let mut invalid = funded_fixture(case).await;
        let before = funded_state(&mut invalid).await;
        let calls = CPI_CALLS.load(Ordering::SeqCst);
        let result = funded_attempt(&mut invalid, 3).await;
        assert!(result.contains(&format!("Custom({code})")), "{result}");
        assert_eq!(CPI_CALLS.load(Ordering::SeqCst), calls);
        assert_eq!(funded_state(&mut invalid).await, before);
    }
    for (identity, code) in [
        (IdentityMismatch::Program, 2012), // Anchor ConstraintAddress.
        (IdentityMismatch::ConfigAddress, 7003),
        (IdentityMismatch::ConfigOwner, 7003),
        (IdentityMismatch::FeeDestination, 7003),
    ] {
        let mut invalid = funded_fixture(FundedCase {
            identity,
            ..FundedCase::default()
        })
        .await;
        let before = funded_state(&mut invalid).await;
        let calls = CPI_CALLS.load(Ordering::SeqCst);
        let result = funded_attempt(&mut invalid, 3).await;
        assert!(
            result.contains(&format!("Custom({code})")),
            "profile={PROFILE} mismatch={identity:?}: {result}"
        );
        assert_eq!(CPI_CALLS.load(Ordering::SeqCst), calls, "{identity:?}");
        assert_eq!(
            OTHER_PROGRAM_CALLS.load(Ordering::SeqCst),
            0,
            "{identity:?}"
        );
        assert_eq!(funded_state(&mut invalid).await, before, "{identity:?}");
        println!("IDENTITY_REFUSAL: profile={PROFILE} mismatch={identity:?} error={code} both_programs_not_called state_unchanged");
    }
    assert_eq!(OTHER_PROGRAM_CALLS.load(Ordering::SeqCst), 0);
    println!("IDENTITY_PROFILE: {PROFILE} program={AMM_ID} other_program={OTHER_AMM_ID}; 4 mismatched identities refused before Raydium");
    println!(
        "RAYDIUM_BACKEND: {}",
        if cfg!(feature = "sbf-raydium") {
            "locally compiled pinned Raydium SBF with unchanged original manifest/lock; Beam diagnostic and SPL/ATA native; inner flow evidenced by logs, intermediate deltas not directly inspected"
        } else {
            "compatible native source with direct intermediate assertions"
        }
    );
    println!("SOURCE_GATE: Beam {} -> Raydium 91739d8 source; 2 successful inner Initialize2 executions, terminal rollback, 1 late source failure and {} input refusals plus 4 identity refusals before Raydium; host Solana 2; native SPL Token 7 / ATA 6; Raydium backend identified above.", option_env!("BEAM_SOURCE_HEAD").unwrap_or("unrecorded"), if cfg!(feature = "mint-authority-guard") { 11 } else { 9 });
}
