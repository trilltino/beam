//! Host-only Beam entrypoint -> synthetic AMM v4 CPI diagnostic.
//! No original Pump, deployed Raydium binary, or real SPL processor runs here.
//! Sources contain preseeded tokens; the probe does not fund them from a curve.
#![cfg(feature = "amm-v4-probe")]

use std::sync::atomic::{AtomicUsize, Ordering};

use anchor_lang::solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::{clock::Clock, Sysvar},
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
const MOCK_REJECT: u32 = 61_000;
const DEVNET_AMM_OWNER: Pubkey = pubkey!("DRayqG9RXYi8WHgWEmRQGrUWRWbhjYWYkCRJDd6JBBak");
static CPI_CALLS: AtomicUsize = AtomicUsize::new(0);

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
fn lp_mint_bytes(authority: Pubkey) -> Vec<u8> {
    let mut data = mint_bytes(6);
    data[..4].copy_from_slice(&1u32.to_le_bytes());
    put_key(&mut data, 4, authority);
    put_u64(&mut data, 36, 1_000_000);
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
fn curve_bytes() -> Vec<u8> {
    let curve = BondingCurve {
        virtual_token_reserves: 0,
        virtual_quote_reserves: 0,
        real_token_reserves: 0,
        real_quote_reserves: 0,
        token_total_supply: 0,
        complete: true,
        creator: Pubkey::default(),
        is_mayhem_mode: false,
        is_cashback_coin: false,
        quote_mint: Pubkey::default(),
        creator_fee_bps: 0,
        can_edit_creator_fee: false,
        is_holder_reward: false,
        creator_fee: 0,
        protocol_fees: 0,
        depth: 0,
        initial_virtual_quote_reserves: 0,
        post_complete_base_out: 0,
        post_complete_quote_in: 0,
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

fn beam_processor<'a>(id: &SdkPubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    // SAFETY: synchronous ProgramTest dispatch; entry does not retain the slice.
    let infos: &'a [AccountInfo<'a>] = unsafe { std::mem::transmute(infos) };
    beam_program::entry(id, infos, data)
}

// These processors exist only in this local bank. In particular, this token
// processor is not SPL Token: it writes a precomputed valid post-state to
// exercise Beam's snapshot and terminal rollback, without claiming genuine
// Raydium settlement or token-transfer authorization.
fn mock_token<'a>(id: &SdkPubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    if anchor(*id) != SPL_TOKEN_PROGRAM_ID
        || data != [99]
        || infos.len() != 8
        || !infos[..6].iter().all(|info| info.owner == &anchor(*id))
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let pool_authority = *infos[6].key;
    let wallet = *infos[7].key;
    infos[0]
        .try_borrow_mut_data()?
        .copy_from_slice(&lp_mint_bytes(pool_authority));
    infos[1]
        .try_borrow_mut_data()?
        .copy_from_slice(&token_bytes(MINT, pool_authority, BASE, false));
    let native_reserve = infos[2].lamports();
    let mut quote_vault = token_bytes(WSOL, pool_authority, QUOTE, true);
    put_u64(&mut quote_vault, 113, native_reserve);
    infos[2]
        .try_borrow_mut_data()?
        .copy_from_slice(&quote_vault);
    infos[3]
        .try_borrow_mut_data()?
        .copy_from_slice(&token_bytes(MINT, wallet, 0, false));
    infos[4]
        .try_borrow_mut_data()?
        .copy_from_slice(&token_bytes(WSOL, wallet, 0, true));
    infos[5]
        .try_borrow_mut_data()?
        .copy_from_slice(&token_bytes(*infos[0].key, wallet, 1_000_000, false));
    **infos[4].try_borrow_mut_lamports()? -= QUOTE;
    **infos[2].try_borrow_mut_lamports()? += QUOTE;
    Ok(())
}

fn mock_ata<'a>(id: &SdkPubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    if anchor(*id) != ASSOCIATED_TOKEN_PROGRAM_ID
        || !data.is_empty()
        || infos.len() != 5
        || !infos[0].is_signer
        || infos[3].key != &SPL_TOKEN_PROGRAM_ID
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let seeds = &[
        infos[0].key.as_ref(),
        infos[3].key.as_ref(),
        infos[4].key.as_ref(),
    ];
    let (ata, bump) = Pubkey::find_program_address(seeds, &anchor(*id));
    if infos[1].key != &ata {
        return Err(ProgramError::InvalidAccountData);
    }
    let ix = system_instruction::create_account(
        infos[0].key,
        infos[1].key,
        Rent::default().minimum_balance(165),
        165,
        &SPL_TOKEN_PROGRAM_ID,
    );
    invoke_signed(
        &ix,
        &[infos[0].clone(), infos[1].clone(), infos[2].clone()],
        &[&[seeds[0], seeds[1], seeds[2], &[bump]]],
    )
}

fn create_amm_account<'a>(
    id: &SdkPubkey,
    infos: &[AccountInfo<'a>],
    index: usize,
    suffix: &[u8],
    space: usize,
    owner: Pubkey,
) -> ProgramResult {
    let market = *infos[14].key;
    let (expected, bump) =
        Pubkey::find_program_address(&[id.as_ref(), market.as_ref(), suffix], &anchor(*id));
    if infos[index].key != &expected {
        return Err(ProgramError::InvalidAccountData);
    }
    let ix = system_instruction::create_account(
        infos[15].key,
        infos[index].key,
        Rent::default().minimum_balance(space),
        space as u64,
        &owner,
    );
    invoke_signed(
        &ix,
        &[infos[15].clone(), infos[index].clone(), infos[2].clone()],
        &[&[id.as_ref(), market.as_ref(), suffix, &[bump]]],
    )
}

fn populate_pool(
    data: &mut [u8],
    id: &SdkPubkey,
    infos: &[AccountInfo<'_>],
    open_time: u64,
    status: u64,
) {
    let p = derive_pool_addresses(&anchor(*id), infos[14].key).unwrap();
    put_u64(data, 0, status);
    put_u64(data, 8, u64::from(p.nonce));
    put_u64(data, 32, 6);
    put_u64(data, 40, 9);
    put_u64(data, 48, 1);
    put_u64(data, 120, 1_000_000_000);
    for (at, value) in [
        (128, 5),
        (136, 10_000),
        (144, 25),
        (152, 10_000),
        (160, 12),
        (168, 100),
        (176, 25),
        (184, 10_000),
    ] {
        put_u64(data, at, value);
    }
    put_u64(data, 224, open_time);
    for (at, key) in [
        (336, p.coin_vault),
        (368, p.pc_vault),
        (400, MINT),
        (432, WSOL),
        (464, p.lp_mint),
        (528, *infos[14].key),
        (592, p.target_orders),
        (688, DEVNET_AMM_OWNER),
    ] {
        put_key(data, at, key);
    }
    put_u64(data, 720, 2_000_000);
}

fn mock_amm<'a>(id: &SdkPubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    if anchor(*id) != DEVNET_PROGRAM_ID
        || infos.len() != 19
        || data.len() != 26
        || data[0] != 1
        || !infos[15].is_signer
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    CPI_CALLS.fetch_add(1, Ordering::SeqCst);
    if data[2] == 0 {
        return Err(ProgramError::Custom(MOCK_REJECT));
    }

    // The synthetic AMM spends from Beam's signed PDA and creates its pool.
    // Branch 1 leaves incomplete state so Beam's post-CPI snapshot rejects.
    create_amm_account(id, infos, 4, b"amm_associated_seed", 752, anchor(*id))?;
    if data[2] == 1 {
        return Ok(());
    }

    // Branch 2 fabricates a complete, internally consistent post-state using
    // test-only ATA/token processors. It does not execute SPL or Raydium code.
    for (index, suffix, space, owner) in [
        (
            6,
            b"lp_mint_associated_seed".as_slice(),
            82,
            SPL_TOKEN_PROGRAM_ID,
        ),
        (
            9,
            b"coin_vault_associated_seed".as_slice(),
            165,
            SPL_TOKEN_PROGRAM_ID,
        ),
        (
            10,
            b"pc_vault_associated_seed".as_slice(),
            165,
            SPL_TOKEN_PROGRAM_ID,
        ),
        (11, b"target_associated_seed".as_slice(), 2208, anchor(*id)),
    ] {
        create_amm_account(id, infos, index, suffix, space, owner)?;
    }
    let ata_ix = Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*infos[15].key, true),
            AccountMeta::new(*infos[18].key, false),
            AccountMeta::new_readonly(*infos[2].key, false),
            AccountMeta::new_readonly(*infos[0].key, false),
            AccountMeta::new_readonly(*infos[6].key, false),
        ],
        data: vec![],
    };
    invoke(
        &ata_ix,
        &[
            infos[15].clone(),
            infos[18].clone(),
            infos[2].clone(),
            infos[0].clone(),
            infos[6].clone(),
            infos[1].clone(),
        ],
    )?;
    let token_ix = Instruction {
        program_id: SPL_TOKEN_PROGRAM_ID,
        accounts: [6, 9, 10, 16, 17, 18, 5, 15]
            .into_iter()
            .map(|index| AccountMeta {
                pubkey: *infos[index].key,
                is_signer: false,
                is_writable: index <= 18 && matches!(index, 6 | 9 | 10 | 16 | 17 | 18),
            })
            .collect(),
        data: vec![99],
    };
    let token_infos: Vec<_> = [6, 9, 10, 16, 17, 18, 5, 15]
        .into_iter()
        .map(|index| infos[index].clone())
        .chain(std::iter::once(infos[0].clone()))
        .collect();
    invoke(&token_ix, &token_infos)?;
    let open_time = u64::from_le_bytes(data[2..10].try_into().unwrap());
    let status = if open_time > Clock::get()?.unix_timestamp as u64 {
        7
    } else {
        6
    };
    populate_pool(
        &mut infos[4].try_borrow_mut_data()?,
        id,
        infos,
        open_time,
        status,
    );
    put_key(&mut infos[11].try_borrow_mut_data()?, 0, *infos[4].key);
    Ok(())
}

struct Fixture {
    bank: ProgramTestContext,
    admin: Keypair,
    instruction: SdkInstruction,
    pool: SdkPubkey,
    source: SdkPubkey,
    signer: SdkPubkey,
}

async fn fixture(precreated_pool: bool, frozen_source: bool) -> Fixture {
    let admin = Keypair::new();
    let (signer, _) =
        Pubkey::find_program_address(&[b"pool-authority", MINT.as_ref()], &beam_program::ID);
    let market =
        Pubkey::find_program_address(&[b"amm-v4-market", MINT.as_ref()], &beam_program::ID).0;
    let pool = derive_pool_addresses(&DEVNET_PROGRAM_ID, &market).unwrap();
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
            open_time: 0,
            init_coin_amount: BASE,
            init_pc_amount: QUOTE,
        },
    )
    .unwrap();
    let curve =
        Pubkey::find_program_address(&[b"bonding-curve", MINT.as_ref()], &beam_program::ID).0;
    let global = Pubkey::find_program_address(&[b"global"], &beam_program::ID).0;
    let mut bank = ProgramTest::new(
        "beam_program",
        sdk(beam_program::ID),
        processor!(beam_processor),
    );
    bank.add_program(
        "synthetic_amm_v4",
        sdk(DEVNET_PROGRAM_ID),
        processor!(mock_amm),
    );
    bank.add_program(
        "synthetic_spl_token",
        sdk(SPL_TOKEN_PROGRAM_ID),
        processor!(mock_token),
    );
    bank.add_program(
        "synthetic_ata",
        sdk(ASSOCIATED_TOKEN_PROGRAM_ID),
        processor!(mock_ata),
    );
    bank.add_account(
        sdk(global),
        account(
            beam_program::ID,
            100_000_000,
            global_bytes(anchor(admin.pubkey())),
            false,
        ),
    );
    bank.add_account(
        sdk(curve),
        account(beam_program::ID, 100_000_000, curve_bytes(), false),
    );
    bank.add_account(
        sdk(signer),
        account(system_program::ID, 100_000_000, vec![], false),
    );
    bank.add_account(
        sdk(MINT),
        account(SPL_TOKEN_PROGRAM_ID, 100_000_000, mint_bytes(6), false),
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
                u64::from(precreated_pool && index == 4),
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
            16 => {
                let mut d = token_bytes(MINT, signer, BASE, false);
                if frozen_source {
                    d[108] = 2;
                }
                (SPL_TOKEN_PROGRAM_ID, 100_000_000, d, false)
            }
            17 => (
                SPL_TOKEN_PROGRAM_ID,
                NATIVE_RENT + QUOTE,
                token_bytes(WSOL, signer, QUOTE, true),
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
        SdkMeta::new_readonly(admin.pubkey(), true),
        SdkMeta::new_readonly(sdk(MINT), false),
        SdkMeta::new_readonly(sdk(curve), false),
        SdkMeta::new(sdk(signer), false),
        SdkMeta::new_readonly(sdk(DEVNET_PROGRAM_ID), false),
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
        data: beam_program::instruction::ProbeAmmV4Initialize2 { open_time: 0 }.data(),
    };
    Fixture {
        bank: bank.start_with_context().await,
        admin,
        instruction,
        pool: sdk(pool.amm),
        source: sdk(sources.user_coin_source),
        signer: sdk(signer),
    }
}

async fn attempt(f: &mut Fixture, open_time: u64) -> String {
    f.instruction.data = beam_program::instruction::ProbeAmmV4Initialize2 { open_time }.data();
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

async fn fixture_state(f: &mut Fixture) -> Vec<Option<Account>> {
    let keys = [4, 6, 9, 10, 11, 13, 15, 16, 17, 18]
        .into_iter()
        .map(|index| f.instruction.accounts[6 + index].pubkey);
    let mut state = Vec::new();
    for key in keys {
        state.push(f.bank.banks_client.get_account(key).await.unwrap());
    }
    state
}

#[tokio::test]
async fn beam_probe_reaches_cpi_and_rejects_before_or_after_without_committing() {
    CPI_CALLS.store(0, Ordering::SeqCst);
    let mut f = fixture(false, false).await;
    let before_signer = f
        .bank
        .banks_client
        .get_account(f.signer)
        .await
        .unwrap()
        .unwrap();
    let first = attempt(&mut f, 0).await;
    assert!(first.contains(&format!("Custom({MOCK_REJECT})")), "{first}");
    assert_eq!(CPI_CALLS.load(Ordering::SeqCst), 1);
    assert!(f
        .bank
        .banks_client
        .get_account(f.pool)
        .await
        .unwrap()
        .is_none());

    let second = attempt(&mut f, 1).await;
    assert!(
        second.contains("Custom(7006)"),
        "snapshot should reject: {second}"
    );
    assert_eq!(CPI_CALLS.load(Ordering::SeqCst), 2);
    assert!(f
        .bank
        .banks_client
        .get_account(f.pool)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        f.bank
            .banks_client
            .get_account(f.signer)
            .await
            .unwrap()
            .unwrap(),
        before_signer
    );

    let before_full = fixture_state(&mut f).await;
    let full = attempt(&mut f, 2).await;
    assert!(
        full.contains("Custom(7002)"),
        "valid synthetic snapshot must reach unconditional gate: {full}"
    );
    assert_eq!(CPI_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(fixture_state(&mut f).await, before_full);

    for (precreated, frozen) in [(true, false), (false, true)] {
        let mut invalid = fixture(precreated, frozen).await;
        let before = CPI_CALLS.load(Ordering::SeqCst);
        let result = attempt(&mut invalid, 0).await;
        assert!(
            result.contains("Custom(7003)"),
            "preflight should reject: {result}"
        );
        assert_eq!(CPI_CALLS.load(Ordering::SeqCst), before);
        let source = invalid
            .bank
            .banks_client
            .get_account(invalid.source)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source.data[108], if frozen { 2 } else { 1 });
    }
}
