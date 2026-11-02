//! Local-only source compatibility gate; always rolls back.
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
        SPL_TOKEN_PROGRAM_ID,
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
}

#[derive(Clone, Copy, Default)]
struct FundedCase {
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
    let pool = derive_pool_addresses(&DEVNET_PROGRAM_ID, &market).unwrap();
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
    let inner = initialize2_instruction(
        DEVNET_PROGRAM_ID,
        &sources,
        Initialize2Args {
            open_time: 3,
            init_coin_amount: base,
            init_pc_amount: QUOTE,
        },
    )
    .unwrap();
    let mut bank = ProgramTest::default();
    // Never select an ambient .so via BPF_OUT_DIR/SBF_OUT_DIR.
    bank.prefer_bpf(false);
    bank.add_program(
        "beam_program",
        sdk(beam_program::ID),
        processor!(beam_processor),
    );
    bank.add_program(
        "pinned_raydium_amm_v4",
        sdk(DEVNET_PROGRAM_ID),
        processor!(pinned_raydium),
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
                (DEVNET_PROGRAM_ID, 100_000_000, d, false)
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
        SdkMeta::new_readonly(sdk(DEVNET_PROGRAM_ID), false),
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
    Fixture {
        bank: bank.start_with_context().await,
        admin,
        instruction,
        pool: sdk(pool.amm),
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
    format!(
        "{:?}",
        f.bank
            .banks_client
            .process_transaction(tx)
            .await
            .unwrap_err()
    )
}

async fn funded_state(f: &mut Fixture) -> Vec<Option<Account>> {
    let indexes = [1, 3, 4, 5, 6, 8, 9, 17, 19, 22, 23, 24, 26, 28, 29, 30, 31];
    let mut state = Vec::new();
    for index in indexes {
        state.push(
            f.bank
                .banks_client
                .get_account(f.instruction.accounts[index].pubkey)
                .await
                .unwrap(),
        );
    }
    state
}

#[tokio::test]
async fn beam_funded_diagnostic_reaches_compatible_pinned_source_and_always_rolls_back() {
    CPI_CALLS.store(0, Ordering::SeqCst);
    SOURCE_SUCCESSES.store(0, Ordering::SeqCst);
    LATE_SOURCE_FAILURES.store(0, Ordering::SeqCst);
    assert_eq!(raydium_amm::id(), DEVNET_PROGRAM_ID);
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
    println!("SOURCE_GATE: Beam {} -> Raydium 91739d8 source; 2 successful inner Initialize2 executions, terminal rollback, 1 late source failure and {} early refusals; Solana 2 dependency adaptation; native SPL Token 7 / ATA 6.", option_env!("BEAM_SOURCE_HEAD").unwrap_or("unrecorded"), if cfg!(feature = "mint-authority-guard") { 11 } else { 9 });
}
