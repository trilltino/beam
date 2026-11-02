//! Adversarial, synthetic AMM v4 post-CPI account inspection.
//! ProgramTest registers test-only Beam and Raydium-devnet processors. Token
//! balances are preseeded post-state fixtures; no original Pump, SPL Token or
//! Raydium binary runs. This is not Beam's migration entrypoint or a live CPI.
use anchor_lang::solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    program_error::ProgramError,
    pubkey,
};
use beam_program::{
    beam_amm_v4::{
        derive_pool_addresses, encode_initialize2, initialize2_instruction, user_lp_ata,
        Initialize2Accounts, Initialize2Args, MintKey, ASSOCIATED_TOKEN_PROGRAM_ID,
        DEVNET_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    beam_amm_v4_snapshot::{validate_post_initialize2, AMM_INFO_DATA_LEN, TARGET_ORDERS_DATA_LEN},
};
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account, bpf_loader_upgradeable, pubkey::Pubkey, signature::Signer, system_program,
    sysvar, transaction::Transaction,
};

const BEAM_MINT: Pubkey = Pubkey::new_from_array([42; 32]);
const MARKET_SEED: Pubkey = Pubkey::new_from_array([43; 32]);
const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
const DEVNET_AMM_OWNER: Pubkey = pubkey!("DRayqG9RXYi8WHgWEmRQGrUWRWbhjYWYkCRJDd6JBBak");
const BASE: u64 = 2_000_000;
const QUOTE: u64 = 2_000_000;
const USER_LP: u64 = 1_000_000;
const NATIVE_RENT: u64 = 3_000_000;
const CLOCK_TIME: i64 = 100;
const REPLAY: u32 = 99;
const SNAPSHOT_REJECT: u32 = 100;

fn wallet_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"pool-authority", BEAM_MINT.as_ref()], &beam_program::ID)
}

fn put_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn put_key(data: &mut [u8], offset: usize, key: Pubkey) {
    data[offset..offset + 32].copy_from_slice(key.as_ref());
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn mint_bytes(decimals: u8, supply: u64, authority: Option<Pubkey>) -> Vec<u8> {
    let mut data = vec![0; 82];
    if let Some(key) = authority {
        data[..4].copy_from_slice(&1u32.to_le_bytes());
        put_key(&mut data, 4, key);
    }
    put_u64(&mut data, 36, supply);
    data[44] = decimals;
    data[45] = 1;
    data
}

fn token_bytes(mint: Pubkey, authority: Pubkey, amount: u64, native: bool) -> Vec<u8> {
    let mut data = vec![0; 165];
    put_key(&mut data, 0, mint);
    put_key(&mut data, 32, authority);
    put_u64(&mut data, 64, amount);
    data[108] = 1; // classic SPL AccountState::Initialized
    if native {
        data[109..113].copy_from_slice(&1u32.to_le_bytes());
        put_u64(&mut data, 113, NATIVE_RENT);
    }
    data
}

fn account(owner: Pubkey, lamports: u64, data: Vec<u8>, executable: bool) -> Account {
    Account {
        lamports,
        data,
        owner,
        executable,
        rent_epoch: 0,
    }
}

fn populate_pool(
    data: &mut [u8],
    p: beam_program::beam_amm_v4::PoolAddresses,
    open_time: u64,
    scenario: u64,
) {
    let status = if open_time > CLOCK_TIME as u64 { 7 } else { 6 };
    put_u64(data, 0, if scenario == 300 { 2 } else { status });
    put_u64(data, 8, u64::from(p.nonce));
    put_u64(data, 32, 6);
    put_u64(data, 40, 9);
    put_u64(data, 48, 1);
    put_u64(data, 56, 0);
    put_u64(data, 120, 1_000_000_000);
    for (offset, value) in [
        (128, 5),
        (136, 10_000),
        (144, 25),
        (152, 10_000),
        (160, 12),
        (168, 100),
        (176, 25),
        (184, 10_000),
    ] {
        put_u64(data, offset, value);
    }
    put_u64(data, 224, open_time);
    for (offset, key) in [
        (
            336,
            if scenario == 400 {
                Pubkey::new_from_array([88; 32])
            } else {
                p.coin_vault
            },
        ),
        (368, p.pc_vault),
        (400, BEAM_MINT),
        (432, WSOL_MINT),
        (464, p.lp_mint),
        (496, Pubkey::default()),
        (528, MARKET_SEED),
        (560, Pubkey::default()),
        (592, p.target_orders),
        (688, DEVNET_AMM_OWNER),
    ] {
        put_key(data, offset, key);
    }
    put_u64(data, 720, 2_000_000); // gross sqrt(BASE * QUOTE)
}

// The local Raydium-ID mock writes only pool and target-orders bytes. Pool
// vaults, sources, LP and fee token accounts were seeded into the bank as
// already settled values; the separate PR #33 harness tests their deltas.
fn mock_amm<'a>(id: &Pubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    if *id != DEVNET_PROGRAM_ID
        || infos.len() != 19
        || data.len() != 26
        || data[0] != 1
        || !infos[15].is_signer
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let open_time = read_u64(data, 2);
    let p =
        derive_pool_addresses(id, &MARKET_SEED).map_err(|_| ProgramError::InvalidAccountData)?;
    if infos[4].owner == id {
        populate_pool(
            &mut infos[4].try_borrow_mut_data()?,
            p,
            open_time,
            open_time,
        );
    }
    // A real processor cannot modify a System-owned substituted target. Let
    // Beam's post-CPI owner check reject that fixture instead of relying on
    // the runtime's earlier foreign-owner write rejection.
    if infos[11].owner == id {
        let target = &mut *infos[11].try_borrow_mut_data()?;
        put_key(
            target,
            0,
            if open_time == 500 {
                Pubkey::new_from_array([87; 32])
            } else {
                p.amm
            },
        );
    }
    Ok(())
}

// This is a test-only caller registered under Beam's ID, not beam_program::entry.
fn mock_beam_caller<'a>(id: &Pubkey, infos: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    if *id != beam_program::ID || infos.len() != 21 || data.len() != 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if infos[20].try_borrow_data()?[0] == 1 {
        return Err(ProgramError::Custom(REPLAY));
    }
    let open_time = read_u64(data, 0);
    let args = Initialize2Args {
        open_time,
        init_pc_amount: QUOTE,
        init_coin_amount: BASE,
    };
    let p = derive_pool_addresses(&DEVNET_PROGRAM_ID, &MARKET_SEED)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let mut metas: Vec<AccountMeta> = infos[..19]
        .iter()
        .map(|info| AccountMeta {
            pubkey: *info.key,
            is_signer: false,
            is_writable: info.is_writable,
        })
        .collect();
    metas[15].is_signer = true;
    let ix = Instruction {
        program_id: DEVNET_PROGRAM_ID,
        accounts: metas,
        data: encode_initialize2(p.nonce, args).to_vec(),
    };
    let (_, bump) = wallet_pda();
    invoke_signed(
        &ix,
        infos,
        &[&[b"pool-authority", BEAM_MINT.as_ref(), &[bump]]],
    )?;
    validate_post_initialize2(
        &infos[19],
        &infos[..19],
        &BEAM_MINT,
        &MARKET_SEED,
        args,
        CLOCK_TIME,
    )
    .map_err(|_| ProgramError::Custom(SNAPSHOT_REJECT))?;
    infos[20].try_borrow_mut_data()?[0] = 1;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    PoolOwner,
    PoolLength,
    PoolStatus,
    PoolIdentity,
    TargetOwner,
    TargetLength,
    TargetPoolKey,
    VaultOwner,
    VaultMint,
    VaultAuthority,
    SourceAuthority,
    LpMintAuthority,
    FeeOwner,
    FeeMint,
    FeeBacking,
    FeeKey,
    VaultSourceAlias,
    MarketSeed,
}

struct Fixture {
    bank: ProgramTestContext,
    outer: Instruction,
    pool: Pubkey,
    target: Pubkey,
    receipt: Pubkey,
    rogue_fee: Pubkey,
    rogue_market: Pubkey,
    base_source: Pubkey,
    keys: Vec<Pubkey>,
}

async fn fixture(fault: Fault) -> Fixture {
    let wallet = wallet_pda().0;
    let p = derive_pool_addresses(&DEVNET_PROGRAM_ID, &MARKET_SEED).unwrap();
    let accounts = Initialize2Accounts {
        coin_mint: MintKey {
            key: BEAM_MINT,
            owner: SPL_TOKEN_PROGRAM_ID,
        },
        pc_mint: MintKey {
            key: WSOL_MINT,
            owner: SPL_TOKEN_PROGRAM_ID,
        },
        market_seed: MARKET_SEED,
        user_wallet: wallet,
        user_coin_source: user_lp_ata(&wallet, &BEAM_MINT),
        user_pc_source: user_lp_ata(&wallet, &WSOL_MINT),
        user_lp_ata: user_lp_ata(&wallet, &p.lp_mint),
    };
    let mut outer = initialize2_instruction(
        DEVNET_PROGRAM_ID,
        &accounts,
        Initialize2Args {
            open_time: 0,
            init_pc_amount: QUOTE,
            init_coin_amount: BASE,
        },
    )
    .unwrap();
    outer.program_id = beam_program::ID;
    outer.accounts[15].is_signer = false;
    let mut bank = ProgramTest::new(
        "test_only_beam_snapshot",
        beam_program::ID,
        processor!(mock_beam_caller),
    );
    let rogue_fee = Pubkey::new_from_array([82; 32]);
    let rogue_market = Pubkey::new_from_array([83; 32]);
    let receipt = Pubkey::new_from_array([84; 32]);
    bank.add_program("test_only_amm_v4", DEVNET_PROGRAM_ID, processor!(mock_amm));
    for (index, meta) in outer.accounts.iter().enumerate() {
        if matches!(index, 2 | 3) {
            continue;
        } // native system/rent
        let owner = match index {
            0 | 1 => bpf_loader_upgradeable::ID,
            4 => {
                if matches!(fault, Fault::PoolOwner) {
                    system_program::ID
                } else {
                    DEVNET_PROGRAM_ID
                }
            }
            7 | 8 | 9 | 10 | 13 | 16 | 17 | 18 | 6 => {
                if (index == 9 && matches!(fault, Fault::VaultOwner))
                    || (index == 13 && matches!(fault, Fault::FeeOwner))
                {
                    system_program::ID
                } else {
                    SPL_TOKEN_PROGRAM_ID
                }
            }
            11 => {
                if matches!(fault, Fault::TargetOwner) {
                    system_program::ID
                } else {
                    DEVNET_PROGRAM_ID
                }
            }
            12 => DEVNET_PROGRAM_ID,
            _ => system_program::ID,
        };
        let data = match index {
            4 => vec![
                0;
                if matches!(fault, Fault::PoolLength) {
                    AMM_INFO_DATA_LEN - 1
                } else {
                    AMM_INFO_DATA_LEN
                }
            ],
            6 => mint_bytes(
                6,
                USER_LP,
                Some(if matches!(fault, Fault::LpMintAuthority) {
                    Pubkey::new_from_array([91; 32])
                } else {
                    p.authority
                }),
            ),
            7 => mint_bytes(6, BASE, None),
            8 => mint_bytes(9, 0, None),
            9 => token_bytes(
                if matches!(fault, Fault::VaultMint) {
                    WSOL_MINT
                } else {
                    BEAM_MINT
                },
                if matches!(fault, Fault::VaultAuthority) {
                    wallet
                } else {
                    p.authority
                },
                BASE,
                false,
            ),
            10 => token_bytes(WSOL_MINT, p.authority, QUOTE, true),
            11 => vec![
                0;
                if matches!(fault, Fault::TargetLength) {
                    TARGET_ORDERS_DATA_LEN - 1
                } else {
                    TARGET_ORDERS_DATA_LEN
                }
            ],
            12 => {
                let mut d = vec![0; 544];
                put_u64(&mut d, 536, 50_000);
                d
            }
            13 => token_bytes(
                if matches!(fault, Fault::FeeMint) {
                    BEAM_MINT
                } else {
                    WSOL_MINT
                },
                Pubkey::new_from_array([85; 32]),
                50_000,
                true,
            ),
            16 => token_bytes(
                BEAM_MINT,
                if matches!(fault, Fault::SourceAuthority) {
                    p.authority
                } else {
                    wallet
                },
                0,
                false,
            ),
            17 => token_bytes(WSOL_MINT, wallet, 0, true),
            18 => token_bytes(p.lp_mint, wallet, USER_LP, false),
            _ => Vec::new(),
        };
        let lamports = match index {
            10 => NATIVE_RENT + QUOTE,
            13 => NATIVE_RENT + 50_000 + u64::from(matches!(fault, Fault::FeeBacking)),
            _ => NATIVE_RENT,
        };
        bank.add_account(meta.pubkey, account(owner, lamports, data, index <= 1));
    }
    bank.add_account(
        rogue_fee,
        account(
            SPL_TOKEN_PROGRAM_ID,
            NATIVE_RENT + 50_000,
            token_bytes(WSOL_MINT, Pubkey::new_from_array([85; 32]), 50_000, true),
            false,
        ),
    );
    bank.add_account(
        rogue_market,
        account(system_program::ID, NATIVE_RENT, Vec::new(), false),
    );
    bank.add_account(
        receipt,
        account(beam_program::ID, NATIVE_RENT, vec![0], false),
    );
    let context = bank.start_with_context().await;
    outer.accounts.extend([
        AccountMeta::new_readonly(DEVNET_PROGRAM_ID, false),
        AccountMeta::new(receipt, false),
    ]);
    let mut keys: Vec<Pubkey> = outer.accounts[..19]
        .iter()
        .map(|meta| meta.pubkey)
        .collect();
    keys.extend([receipt, rogue_fee, rogue_market]);
    Fixture {
        bank: context,
        outer,
        pool: p.amm,
        target: p.target_orders,
        receipt,
        rogue_fee,
        rogue_market,
        base_source: accounts.user_coin_source,
        keys,
    }
}

fn tx(f: &Fixture, fault: Fault, open_time: u64) -> Transaction {
    let mut ix = f.outer.clone();
    ix.data = open_time.to_le_bytes().to_vec();
    if matches!(fault, Fault::FeeKey) {
        ix.accounts[13].pubkey = f.rogue_fee;
    }
    if matches!(fault, Fault::VaultSourceAlias) {
        ix.accounts[9].pubkey = f.base_source;
    }
    if matches!(fault, Fault::MarketSeed) {
        ix.accounts[14].pubkey = f.rogue_market;
    }
    Transaction::new_signed_with_payer(
        &[ix],
        Some(&f.bank.payer.pubkey()),
        &[&f.bank.payer],
        f.bank.last_blockhash,
    )
}

async fn state(f: &Fixture) -> Vec<Account> {
    let mut out = Vec::with_capacity(f.keys.len());
    for key in &f.keys {
        out.push(
            f.bank
                .banks_client
                .get_account(*key)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    out
}

#[tokio::test]
async fn snapshot_accepts_immediate_and_waiting_trade_without_real_raydium() {
    for open_time in [0, 200] {
        let f = fixture(Fault::None).await;
        f.bank
            .banks_client
            .process_transaction(tx(&f, Fault::None, open_time))
            .await
            .unwrap();
        let pool = f
            .bank
            .banks_client
            .get_account(f.pool)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            read_u64(&pool.data, 0),
            if open_time > CLOCK_TIME as u64 { 7 } else { 6 }
        );
        assert_eq!(read_u64(&pool.data, 720), BASE); // gross LP state, user owns BASE - 10^6
        let target = f
            .bank
            .banks_client
            .get_account(f.target)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&target.data[..32], f.pool.as_ref());
        let receipt = f
            .bank
            .banks_client
            .get_account(f.receipt)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt.data[0], 1);
        let after = state(&f).await;
        let replay = f
            .bank
            .banks_client
            .process_transaction(tx(&f, Fault::None, if open_time == 0 { 200 } else { 0 }))
            .await
            .unwrap_err();
        assert!(format!("{replay:?}").contains(&format!("Custom({REPLAY})")));
        assert_eq!(state(&f).await, after);
    }
}

#[tokio::test]
async fn malformed_post_state_rejects_and_rolls_back_mock_pool_and_target_writes() {
    for fault in [
        Fault::PoolOwner,
        Fault::PoolLength,
        Fault::PoolStatus,
        Fault::PoolIdentity,
        Fault::TargetOwner,
        Fault::TargetLength,
        Fault::TargetPoolKey,
        Fault::VaultOwner,
        Fault::VaultMint,
        Fault::VaultAuthority,
        Fault::SourceAuthority,
        Fault::LpMintAuthority,
        Fault::FeeOwner,
        Fault::FeeMint,
        Fault::FeeBacking,
        Fault::FeeKey,
        Fault::VaultSourceAlias,
        Fault::MarketSeed,
    ] {
        let f = fixture(fault).await;
        let before = state(&f).await;
        let open_time = match fault {
            Fault::PoolStatus => 300,
            Fault::PoolIdentity => 400,
            Fault::TargetPoolKey => 500,
            _ => 0,
        };
        let result = f
            .bank
            .banks_client
            .process_transaction(tx(&f, fault, open_time))
            .await;
        let error = result.expect_err("fault unexpectedly passed");
        assert!(
            format!("{error:?}").contains(&format!("Custom({SNAPSHOT_REJECT})")),
            "{fault:?} failed before post-CPI validation: {error:?}"
        );
        assert_eq!(state(&f).await, before, "{fault:?} did not roll back");
    }
}
