//! Beam's `migrate_amm_v4` handler, built to SBF, calling the pinned Raydium
//! AMM v4 `Initialize2` processor in one local banks transaction.
//!
//! Beam runs as its compiled SBF program, so these tests need two builds: the
//! local-test one (activation feature on) and the production-style one (off),
//! which only `production_build_cannot_enable_migration` loads:
//!
//! ```sh
//! (cd programs/beam && cargo build-sbf --arch v0 --no-default-features --features no-idl,experimental-amm-v4)
//! (cd programs/beam && cargo build-sbf --arch v0 --no-default-features --features no-idl --sbf-out-dir target/deploy-production)
//! ```
//!
//! Raydium runs as the pinned Rust source processor, not deployed bytecode.
//! This does not enable graduation: production builds cannot set `enabled`.
//!
//! Run with `cargo test --locked --features beam-sbf --test beam_migration`.
#![cfg(feature = "beam-sbf")]

use beam_amm_v4_validator::{derive_pool, expected_lp, pinned_config_bytes, process_raydium};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::program_pack::Pack;
use solana_program_test::{processor, tokio, BanksClientError, ProgramTest, ProgramTestContext};
use solana_pubkey::{pubkey, Pubkey};
use solana_rent::Rent;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

const BEAM: Pubkey = pubkey!("GdaQ7RHbGKGEL5Px66tQP78S67XcgxYAzFUwja71qSJP");
const SYSTEM: Pubkey = pubkey!("11111111111111111111111111111111");
const CURVE_CREATOR: Pubkey = pubkey!("Stake11111111111111111111111111111111111111");
/// `sha256("account:AmmV4Rights")[..8]`, the discriminator draft PR #49 reads.
const DISC_RIGHTS: [u8; 8] = [88, 155, 79, 159, 221, 232, 159, 98];

// Anchor discriminators: sha256("global:<ix>" or "account:<Type>")[..8].
const DISC_CONFIGURE: [u8; 8] = [144, 16, 143, 164, 78, 235, 65, 59];
const DISC_SET_ENABLED: [u8; 8] = [171, 118, 93, 180, 198, 99, 182, 159];
const DISC_MIGRATE: [u8; 8] = [10, 219, 107, 168, 193, 191, 50, 99];
const DISC_GLOBAL: [u8; 8] = [167, 232, 232, 177, 200, 108, 114, 127];
const DISC_CURVE: [u8; 8] = [23, 183, 248, 55, 96, 216, 172, 96];
const DISC_RECEIPT: [u8; 8] = [179, 123, 65, 124, 38, 208, 232, 159];

// BeamError offset 7000.
const ERR_NOT_ENABLED: u32 = 7002;
const ERR_INVALID_ACCOUNT: u32 = 7003;
const ERR_FEE_CAP: u32 = 7007;

const DECIMALS: u8 = 6;
const TOTAL_SUPPLY: u64 = 1_000_000_000_000_000;
const VAULT_TOKENS: u64 = 206_900_000_000_000;
const REAL_QUOTE: u64 = 85_000_000_000;
const POST_COMPLETE_QUOTE: u64 = 1_000_000;
const MIGRATION_FEE: u64 = 20_000_000;
const CREATOR_FEE: u64 = 7_000;
const PROTOCOL_FEES: u64 = 11_000;
const CURVE_SURPLUS: u64 = 777;
const CREATE_FEE: u64 = 25_000_000;
const FEE_DEST_DONATION: u64 = 999;
const AUTHORITY_DONATION: u64 = 1_000_000;
const BASE_DONATION: u64 = 50;
const WSOL_DONATION: u64 = 80;
const ERR_SETUP_BOUNDS: u32 = 7008;

fn rent() -> Rent {
    Rent::default()
}

/// What `Initialize2` takes from the wallet: the fee plus rent for the six accounts it creates.
fn setup_cost(fee: u64) -> u64 {
    fee + rent().minimum_balance(AMM_INFO_LEN)
        + rent().minimum_balance(TARGET_ORDERS_LEN)
        + rent().minimum_balance(Mint::LEN)
        + 3 * rent().minimum_balance(TokenAccount::LEN)
}

/// The exact setup funding for the standard fixture, so nothing is refunded.
fn budget() -> u64 {
    setup_cost(CREATE_FEE)
}

fn mint_account(owner: &Pubkey, decimals: u8, supply: u64) -> Account {
    let mut mint = Mint::default();
    mint.decimals = decimals;
    mint.is_initialized = true;
    mint.supply = supply;
    let mut data = vec![0; Mint::LEN];
    Mint::pack(mint, &mut data).unwrap();
    Account { lamports: rent().minimum_balance(data.len()), data, owner: *owner, executable: false, rent_epoch: 0 }
}

fn token_account(mint: &Pubkey, owner: &Pubkey, amount: u64, native_reserve: Option<u64>) -> Account {
    let mut token = TokenAccount::default();
    token.mint = *mint;
    token.owner = *owner;
    token.amount = amount;
    token.state = AccountState::Initialized;
    token.is_native = native_reserve.into();
    let mut data = vec![0; TokenAccount::LEN];
    TokenAccount::pack(token, &mut data).unwrap();
    let lamports = match native_reserve {
        Some(reserve) => reserve + amount,
        None => rent().minimum_balance(data.len()),
    };
    Account { lamports, data, owner: spl_token::id(), executable: false, rent_epoch: 0 }
}

fn put_key(d: &mut Vec<u8>, k: &Pubkey) {
    d.extend_from_slice(k.as_ref());
}
fn put_u64(d: &mut Vec<u8>, v: u64) {
    d.extend_from_slice(&v.to_le_bytes());
}

/// Anchor-serialized `Global`, in the field order of `programs/beam/src/lib.rs`.
fn global_bytes(authority: &Pubkey, withdraw: &Pubkey) -> Vec<u8> {
    let zero = Pubkey::default();
    let mut d = DISC_GLOBAL.to_vec();
    d.push(1);
    put_key(&mut d, authority);
    put_key(&mut d, &zero);
    for v in [1u64, 1, 1, TOTAL_SUPPLY, 100] {
        put_u64(&mut d, v);
    }
    put_key(&mut d, withdraw);
    d.push(1); // enable_migrate
    put_u64(&mut d, MIGRATION_FEE);
    put_u64(&mut d, 0);
    (0..7).for_each(|_| put_key(&mut d, &zero));
    put_key(&mut d, &zero);
    put_key(&mut d, &zero);
    d.push(0);
    put_key(&mut d, &zero);
    put_key(&mut d, &zero);
    d.push(0);
    (0..7).for_each(|_| put_key(&mut d, &zero));
    d.push(0);
    (0..8).for_each(|_| put_key(&mut d, &zero));
    put_u64(&mut d, 0);
    put_u64(&mut d, 0);
    put_key(&mut d, &zero);
    d.push(0);
    put_u64(&mut d, 0);
    put_key(&mut d, &zero);
    d.push(0);
    d.push(0);
    d
}

/// A completed curve: all real tokens sold, quote reserves recorded.
fn curve_bytes() -> Vec<u8> {
    let zero = Pubkey::default();
    let mut d = DISC_CURVE.to_vec();
    for v in [1u64, 1, 0, REAL_QUOTE, TOTAL_SUPPLY] {
        put_u64(&mut d, v);
    }
    d.push(1); // complete
    put_key(&mut d, &CURVE_CREATOR);
    d.extend_from_slice(&[0, 0]); // mayhem, cashback
    put_key(&mut d, &zero); // native SOL quote
    put_u64(&mut d, 0);
    d.extend_from_slice(&[0, 0]);
    put_u64(&mut d, CREATOR_FEE);
    put_u64(&mut d, PROTOCOL_FEES);
    d.push(0);
    for v in [0u64, 0, POST_COMPLETE_QUOTE] {
        put_u64(&mut d, v);
    }
    assert_eq!(d.len(), 8 + 158);
    d
}

#[derive(Clone, Copy)]
struct Opts {
    donations: bool,
    mint_owner: Pubkey,
    precreate_pool: bool,
    /// Load the production-style build (no `experimental-amm-v4`) instead of the local-test one.
    production_binary: bool,
}

impl Opts {
    fn standard() -> Self {
        Self { donations: true, mint_owner: spl_token::id(), precreate_pool: false, production_binary: false }
    }
}

struct Fx {
    mint: Pubkey,
    global: Pubkey,
    withdraw: Pubkey,
    curve: Pubkey,
    curve_vault: Pubkey,
    config: Pubkey,
    authority: Pubkey,
    authority_base: Pubkey,
    authority_wsol: Pubkey,
    authority_lp: Pubkey,
    receipt: Pubkey,
    rights: Pubkey,
    ray: beam_amm_v4_validator::DerivedPool,
    donations: bool,
}

fn pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &BEAM).0
}

fn build(opts: Opts) -> (ProgramTest, Fx) {
    let mint = Pubkey::new_unique();
    let ray = derive_pool(&mint);
    let authority = pda(&[b"amm-v4-migration-authority", mint.as_ref()]);
    let curve = pda(&[b"bonding-curve", mint.as_ref()]);
    let fx = Fx {
        mint,
        global: pda(&[b"global"]),
        withdraw: Pubkey::new_unique(),
        curve,
        curve_vault: spl_associated_token_account::get_associated_token_address(&curve, &mint),
        config: pda(&[b"beam-amm-v4-config"]),
        authority,
        authority_base: spl_associated_token_account::get_associated_token_address(&authority, &mint),
        authority_wsol: spl_associated_token_account::get_associated_token_address(&authority, &spl_token::native_mint::id()),
        authority_lp: spl_associated_token_account::get_associated_token_address(&authority, &ray.lp_mint),
        receipt: pda(&[b"beam-amm-v4-migration", mint.as_ref()]),
        rights: pda(&[b"beam-amm-rights", ray.amm.as_ref()]),
        ray,
        donations: opts.donations,
    };

    let dir = if opts.production_binary {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../programs/beam/target/deploy-production")
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../programs/beam/target/deploy")
    };
    assert!(std::path::Path::new(dir).join("beam_program.so").exists(), "build Beam first (see this file header): {dir}/beam_program.so");
    std::env::set_var("SBF_OUT_DIR", dir);
    let mut pt = ProgramTest::default();
    pt.prefer_bpf(false);
    pt.set_compute_max_units(1_400_000);
    pt.add_upgradeable_program_to_genesis("beam_program", &BEAM);
    pt.add_program("raydium_amm", raydium_amm::id(), processor!(process_raydium));

    let sys = |lamports: u64| Account { lamports, data: vec![], owner: SYSTEM, executable: false, rent_epoch: 0 };
    let program_owned = |data: Vec<u8>| Account { lamports: rent().minimum_balance(data.len()), data, owner: BEAM, executable: false, rent_epoch: 0 };

    let curve_data = curve_bytes();
    let curve_lamports = rent().minimum_balance(curve_data.len()) + CREATOR_FEE + PROTOCOL_FEES + REAL_QUOTE + POST_COMPLETE_QUOTE + CURVE_SURPLUS;
    pt.add_account(fx.curve, Account { lamports: curve_lamports, ..program_owned(curve_data) });
    pt.add_account(fx.mint, mint_account(&opts.mint_owner, DECIMALS, TOTAL_SUPPLY));
    pt.add_account(fx.curve_vault, token_account(&mint, &curve, VAULT_TOKENS, None));
    pt.add_account(spl_token::native_mint::id(), mint_account(&spl_token::id(), 9, 0));
    // Raydium's canonical config, with the pool-creation fee the processor charges.
    let cfg = pinned_config_bytes(&raydium_amm::processor::config_feature::amm_owner::id(), &raydium_amm::processor::config_feature::amm_owner::id(), CREATE_FEE);
    pt.add_account(fx.ray.config, Account { lamports: rent().minimum_balance(cfg.len()), data: cfg, owner: raydium_amm::id(), executable: false, rent_epoch: 0 });
    pt.add_account(
        fx.ray.fee_destination,
        token_account(&spl_token::native_mint::id(), &Pubkey::new_unique(), FEE_DEST_DONATION, Some(rent().minimum_balance(TokenAccount::LEN))),
    );
    if opts.donations {
        pt.add_account(fx.authority, sys(AUTHORITY_DONATION));
        pt.add_account(fx.authority_base, token_account(&mint, &authority, BASE_DONATION, None));
        pt.add_account(fx.authority_wsol, token_account(&spl_token::native_mint::id(), &authority, WSOL_DONATION, Some(rent().minimum_balance(TokenAccount::LEN))));
    }
    if opts.precreate_pool {
        pt.add_account(fx.ray.amm, sys(1));
    }
    (pt, fx)
}

async fn start(pt: ProgramTest, fx: &Fx) -> ProgramTestContext {
    let mut ctx = pt.start_with_context().await;
    // The payer is generated by ProgramTest, so Global names it as authority here.
    let global = global_bytes(&ctx.payer.pubkey(), &fx.withdraw);
    let account = Account { lamports: rent().minimum_balance(global.len()), data: global, owner: BEAM, executable: false, rent_epoch: 0 };
    ctx.set_account(&fx.global, &account.into());
    ctx
}

fn meta(key: Pubkey, writable: bool, signer: bool) -> AccountMeta {
    if writable { AccountMeta::new(key, signer) } else { AccountMeta::new_readonly(key, signer) }
}

fn configure_ix(fx: &Fx, authority: &Pubkey, cap: u64) -> Instruction {
    let mut data = DISC_CONFIGURE.to_vec();
    put_u64(&mut data, cap);
    Instruction { program_id: BEAM, accounts: vec![meta(fx.global, false, false), meta(*authority, true, true), meta(fx.config, true, false), meta(SYSTEM, false, false)], data }
}

fn enable_ix(fx: &Fx, authority: &Pubkey, enabled: bool) -> Instruction {
    let mut data = DISC_SET_ENABLED.to_vec();
    data.push(enabled as u8);
    Instruction { program_id: BEAM, accounts: vec![meta(fx.global, false, false), meta(*authority, false, true), meta(fx.config, true, false)], data }
}

fn migrate_ix(fx: &Fx, user: &Pubkey, max_fee: u64, budget: u64) -> Instruction {
    let mut data = DISC_MIGRATE.to_vec();
    for v in [0u64, max_fee, budget] {
        put_u64(&mut data, v);
    }
    let wsol = spl_token::native_mint::id();
    let accounts = vec![
        meta(fx.global, false, false),
        meta(fx.withdraw, true, false),
        meta(fx.mint, false, false),
        meta(fx.curve, true, false),
        meta(fx.curve_vault, true, false),
        meta(*user, true, true),
        meta(fx.config, false, false),
        meta(raydium_amm::id(), false, false),
        meta(fx.ray.config, false, false),
        meta(fx.authority, true, false),
        meta(fx.authority_base, true, false),
        meta(wsol, false, false),
        meta(fx.authority_wsol, true, false),
        meta(fx.ray.amm, true, false),
        meta(fx.ray.amm_authority, false, false),
        meta(fx.ray.lp_mint, true, false),
        meta(fx.ray.coin_vault, true, false),
        meta(fx.ray.pc_vault, true, false),
        meta(fx.ray.target_orders, true, false),
        meta(fx.ray.fee_destination, true, false),
        meta(fx.authority_lp, true, false),
        meta(fx.receipt, true, false),
        meta(fx.rights, true, false),
        meta(spl_token::id(), false, false),
        meta(spl_associated_token_account::id(), false, false),
        meta(SYSTEM, false, false),
        meta(solana_program::sysvar::rent::ID, false, false),
    ];
    Instruction { program_id: BEAM, accounts, data }
}

async fn send(ctx: &mut ProgramTestContext, ixs: &[Instruction]) -> Result<(), BanksClientError> {
    let blockhash = ctx.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(ixs, Some(&ctx.payer.pubkey()), &[&ctx.payer], blockhash);
    ctx.banks_client.process_transaction(tx).await
}

fn assert_custom(result: Result<(), BanksClientError>, code: u32) {
    let err = result.expect_err("transaction must fail");
    assert!(format!("{err:?}").contains(&format!("Custom({code})")), "expected Custom({code}), got {err:?}");
}

fn assert_failed(result: Result<(), BanksClientError>) {
    result.expect_err("transaction must fail");
}

async fn account(ctx: &mut ProgramTestContext, key: &Pubkey) -> Option<Account> {
    ctx.banks_client.get_account(*key).await.unwrap()
}

fn amount(a: &Account) -> u64 {
    TokenAccount::unpack(&a.data).unwrap().amount
}

/// Every account a migration could touch, for exact rollback comparison.
async fn snapshot(ctx: &mut ProgramTestContext, fx: &Fx) -> Vec<Option<Account>> {
    let keys = [
        fx.curve, fx.curve_vault, fx.withdraw, fx.authority, fx.authority_base, fx.authority_wsol, fx.authority_lp, fx.receipt, fx.rights, fx.config,
        fx.ray.amm, fx.ray.lp_mint, fx.ray.coin_vault, fx.ray.pc_vault, fx.ray.target_orders, fx.ray.fee_destination, fx.mint,
    ];
    let mut out = Vec::new();
    for key in keys {
        out.push(account(ctx, &key).await);
    }
    out
}

async fn enabled_context(opts: Opts, cap: u64) -> (ProgramTestContext, Fx) {
    let (pt, fx) = build(opts);
    let mut ctx = start(pt, &fx).await;
    let payer = ctx.payer.pubkey();
    send(&mut ctx, &[configure_ix(&fx, &payer, cap)]).await.unwrap();
    send(&mut ctx, &[enable_ix(&fx, &payer, true)]).await.unwrap();
    (ctx, fx)
}


fn lamports(a: &Option<Account>) -> u64 {
    a.as_ref().map_or(0, |a| a.lamports)
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

const TARGET_ORDERS_LEN: usize = 2208;
const AMM_INFO_LEN: usize = 752;

/// A migrated pool must be readable by a graduated-ancestor quote: SwapOnly status,
/// no pending PnL, identity fields equal to the addresses Beam derives (the mint is the
/// market seed), vaults owned by Raydium's authority with no delegate or close authority,
/// and a WSOL vault fully backed by lamports. These are the checks the AMM v4 quote adapter
/// in draft PR #49 makes on raw pool bytes; the offsets are pinned in `pool_layout.rs`.
/// The rights record it also needs is written by the migration; see `assert_rights_record`.
async fn assert_quote_ready(ctx: &mut ProgramTestContext, fx: &Fx, coin: u64, pc: u64) {
    let pool = account(ctx, &fx.ray.amm).await.unwrap().data;
    let key_at = |at: usize| Pubkey::try_from(&pool[at..at + 32]).unwrap();
    assert_eq!(u64_at(&pool, 0), 6, "SwapOnly");
    assert_eq!((u64_at(&pool, 192), u64_at(&pool, 200)), (0, 0), "no pending PnL on a fresh pool");
    assert_eq!(key_at(336), fx.ray.coin_vault);
    assert_eq!(key_at(368), fx.ray.pc_vault);
    assert_eq!(key_at(400), fx.mint);
    assert_eq!(key_at(432), spl_token::native_mint::id());
    assert_eq!(key_at(464), fx.ray.lp_mint);
    assert_eq!(key_at(528), fx.mint, "market seed is the mint");
    assert_eq!(key_at(592), fx.ray.target_orders);
    for (vault, mint, amount, native) in [(fx.ray.coin_vault, fx.mint, coin, false), (fx.ray.pc_vault, spl_token::native_mint::id(), pc, true)] {
        let acct = account(ctx, &vault).await.unwrap();
        assert_eq!(acct.owner, spl_token::id());
        let t = TokenAccount::unpack(&acct.data).unwrap();
        assert_eq!((t.mint, t.owner, t.amount, t.state), (mint, fx.ray.amm_authority, amount, AccountState::Initialized));
        assert!(t.delegate.is_none() && t.close_authority.is_none(), "vault must have no delegate or close authority");
        let native_reserve: Option<u64> = t.is_native.into();
        match (native, native_reserve) {
            (false, None) => {}
            (true, Some(reserve)) => assert!(acct.lamports >= reserve + t.amount, "WSOL vault is not fully backed"),
            _ => panic!("unexpected native flag on {vault}"),
        }
    }
}

/// The per-pool rights record, byte for byte what draft PR #49's `register` writes for a
/// fresh pool: its discriminator, then pool, mint, market seed (the mint), creator and payout
/// (both the curve's creator), an unset donation record, revision 0 and not paused.
async fn assert_rights_record(ctx: &mut ProgramTestContext, fx: &Fx) {
    let rights = account(ctx, &fx.rights).await.expect("rights record");
    assert_eq!(rights.owner, BEAM);
    let mut expected = DISC_RIGHTS.to_vec();
    for key in [fx.ray.amm, fx.mint, fx.mint, CURVE_CREATOR, CURVE_CREATOR, Pubkey::default()] {
        put_key(&mut expected, &key);
    }
    put_u64(&mut expected, 0);
    expected.push(0);
    assert_eq!(expected.len(), 209);
    assert_eq!(rights.data, expected);
    assert_eq!(rights.lamports, rent().minimum_balance(209));
}

/// Runs the migration and checks every effect against the planned amounts.
async fn assert_migrates(ctx: &mut ProgramTestContext, fx: &Fx) {
    let before = snapshot(ctx, fx).await;
    let payer = ctx.payer.pubkey();
    send(ctx, &[migrate_ix(fx, &payer, CREATE_FEE, budget())]).await.unwrap();

    let pc = REAL_QUOTE + POST_COMPLETE_QUOTE - MIGRATION_FEE;
    let pool = account(ctx, &fx.ray.amm).await.expect("pool created");
    assert_eq!(pool.owner, raydium_amm::id());
    assert_eq!(pool.data.len(), AMM_INFO_LEN);
    assert_eq!(u64_at(&pool.data, 0), 6, "SwapOnly");
    assert_eq!(amount(&account(ctx, &fx.ray.coin_vault).await.unwrap()), VAULT_TOKENS);
    assert_eq!(amount(&account(ctx, &fx.ray.pc_vault).await.unwrap()), pc);
    assert_quote_ready(ctx, fx, VAULT_TOKENS, pc).await;

    // LP is issued to the migration PDA and stays there: no burn, no transfer.
    let (_, user_lp) = expected_lp(VAULT_TOKENS, pc, DECIMALS).unwrap();
    let lp_mint = Mint::unpack(&account(ctx, &fx.ray.lp_mint).await.unwrap().data).unwrap();
    assert_eq!(lp_mint.supply, user_lp);
    let lp = TokenAccount::unpack(&account(ctx, &fx.authority_lp).await.unwrap().data).unwrap();
    assert_eq!((lp.owner, lp.mint, lp.amount), (fx.authority, fx.ray.lp_mint, user_lp));

    // Raydium's fee lands once; the curve keeps rent, fee claims and the surplus.
    assert_eq!(amount(&account(ctx, &fx.ray.fee_destination).await.unwrap()), FEE_DEST_DONATION + CREATE_FEE);
    assert_eq!(amount(&account(ctx, &fx.curve_vault).await.unwrap()), 0);
    let curve = account(ctx, &fx.curve).await.unwrap();
    assert_eq!(curve.lamports, lamports(&before[0]) - (REAL_QUOTE + POST_COMPLETE_QUOTE));
    assert_eq!(curve.lamports, rent().minimum_balance(curve.data.len()) + CREATOR_FEE + PROTOCOL_FEES + CURVE_SURPLUS);
    assert_eq!(lamports(&account(ctx, &fx.withdraw).await), MIGRATION_FEE);

    // Unrelated donations are neither adopted nor swept, and unspent setup is refunded.
    let (base_d, wsol_d, auth_d) = if fx.donations { (BASE_DONATION, WSOL_DONATION, AUTHORITY_DONATION) } else { (0, 0, 0) };
    assert_eq!(amount(&account(ctx, &fx.authority_base).await.unwrap()), base_d);
    let wsol = account(ctx, &fx.authority_wsol).await.unwrap();
    assert_eq!(amount(&wsol), wsol_d);
    assert_eq!(wsol.lamports, rent().minimum_balance(TokenAccount::LEN) + wsol_d);
    assert_eq!(lamports(&account(ctx, &fx.authority).await), auth_d);

    let r = account(ctx, &fx.receipt).await.expect("receipt");
    assert_eq!(r.owner, BEAM);
    assert_eq!(r.data[..8], DISC_RECEIPT);
    assert_eq!(r.data[8..40], *fx.mint.as_ref());
    assert_eq!(r.data[40..72], *fx.ray.amm.as_ref());
    assert_eq!(r.data[72..104], *fx.ray.lp_mint.as_ref());
    let spent = CREATE_FEE
        + rent().minimum_balance(TARGET_ORDERS_LEN)
        + rent().minimum_balance(Mint::LEN)
        + 2 * rent().minimum_balance(TokenAccount::LEN)
        + rent().minimum_balance(AMM_INFO_LEN)
        + rent().minimum_balance(TokenAccount::LEN);
    let words: Vec<u64> = (0..7).map(|i| u64_at(&r.data, 104 + 8 * i)).collect();
    assert_eq!(words, vec![VAULT_TOKENS, pc, MIGRATION_FEE, CREATE_FEE, user_lp, spent, 0]);
    assert_eq!(r.data[160], 0, "LP disposition is unresolved");
    assert_rights_record(ctx, fx).await;
}

#[tokio::test]
async fn migrates_through_the_real_raydium_processor_and_preserves_donations() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    assert_migrates(&mut ctx, &fx).await;
}

#[tokio::test]
async fn migrates_when_the_source_accounts_do_not_exist_yet() {
    let (mut ctx, fx) = enabled_context(Opts { donations: false, ..Opts::standard() }, CREATE_FEE).await;
    assert_migrates(&mut ctx, &fx).await;
}

#[tokio::test]
async fn disabled_config_blocks_before_any_movement() {
    let (pt, fx) = build(Opts::standard());
    let mut ctx = start(pt, &fx).await;
    let payer = ctx.payer.pubkey();
    send(&mut ctx, &[configure_ix(&fx, &payer, CREATE_FEE)]).await.unwrap();
    let before = snapshot(&mut ctx, &fx).await;
    assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget())]).await, ERR_NOT_ENABLED);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
}

/// The binary a release would use has no `experimental-amm-v4`: the authority cannot
/// switch migration on, so `migrate_amm_v4` stays unreachable even with every account valid.
#[tokio::test]
async fn production_build_cannot_enable_migration() {
    let (pt, fx) = build(Opts { production_binary: true, ..Opts::standard() });
    let mut ctx = start(pt, &fx).await;
    let payer = ctx.payer.pubkey();
    send(&mut ctx, &[configure_ix(&fx, &payer, CREATE_FEE)]).await.unwrap();
    let before = snapshot(&mut ctx, &fx).await;
    assert_custom(send(&mut ctx, &[enable_ix(&fx, &payer, true)]).await, ERR_NOT_ENABLED);
    assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget())]).await, ERR_NOT_ENABLED);
    assert_eq!(snapshot(&mut ctx, &fx).await, before, "a refused enable or migration must change nothing");
    // Turning it off is always allowed.
    send(&mut ctx, &[enable_ix(&fx, &payer, false)]).await.unwrap();
}

#[tokio::test]
async fn replay_is_rejected_and_leaves_the_pool_untouched() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    assert_migrates(&mut ctx, &fx).await;
    let before = snapshot(&mut ctx, &fx).await;
    let payer = ctx.payer.pubkey();
    assert_failed(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget())]).await);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
}

#[tokio::test]
async fn create_pool_fee_must_fit_both_the_caller_and_the_stored_cap() {
    // Caller cap above the stored cap.
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let before = snapshot(&mut ctx, &fx).await;
    assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE + 1, budget())]).await, ERR_FEE_CAP);
    // Raydium's current fee above a caller cap that is within the stored cap.
    assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE - 1, budget())]).await, ERR_FEE_CAP);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
}

#[tokio::test]
async fn token_2022_mint_fails_before_liquidity_moves() {
    let token_2022 = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
    let (mut ctx, fx) = enabled_context(Opts { mint_owner: token_2022, ..Opts::standard() }, CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let before = snapshot(&mut ctx, &fx).await;
    assert_failed(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget())]).await);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
}

/// A funded account at the deterministic pool address blocks migration safely.
/// It also shows the cost of a mint-derived AMM seed: anyone who creates that
/// pool first stops this launch from graduating. See docs/AMM_V4_INTEGRATION.md.
#[tokio::test]
async fn precreated_pool_blocks_migration_without_moving_liquidity() {
    let (mut ctx, fx) = enabled_context(Opts { precreate_pool: true, ..Opts::standard() }, CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let before = snapshot(&mut ctx, &fx).await;
    assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget())]).await, ERR_INVALID_ACCOUNT);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
}

#[tokio::test]
async fn substituted_accounts_are_rejected() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let before = snapshot(&mut ctx, &fx).await;
    // (account index, name, whether Beam's own check reports InvalidRaydiumAccount)
    for (index, name, beam_error) in [
        (7, "amm program", false),
        (8, "amm config", true),
        (13, "pool", true),
        (14, "amm authority", true),
        (15, "lp mint", true),
        (16, "coin vault", true),
        (17, "pc vault", true),
        (18, "target orders", true),
        (19, "fee destination", true),
        (20, "authority lp", true),
    ] {
        let mut ix = migrate_ix(&fx, &payer, CREATE_FEE, budget());
        ix.accounts[index].pubkey = Pubkey::new_unique();
        let result = send(&mut ctx, &[ix]).await;
        if name == "pool" {
            // The rights record's address is derived from the pool, so Anchor's seeds check
            // (ConstraintSeeds, 2006) rejects a substituted pool before Beam's own check runs.
            assert_custom(result, 2006);
        } else if beam_error {
            assert_custom(result, ERR_INVALID_ACCOUNT);
        } else {
            assert_failed(result);
        }
        assert_eq!(snapshot(&mut ctx, &fx).await, before, "state changed after substituting {name}");
    }
}

/// Setup funding must cover the exact cost and may not exceed the worst case the
/// caller accepted. Both sides fail before any lamport or token moves.
#[tokio::test]
async fn setup_funding_must_sit_between_the_exact_cost_and_the_accepted_worst_case() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let before = snapshot(&mut ctx, &fx).await;
    // The create fee alone, and one lamport short of the real cost.
    for short in [CREATE_FEE + 1_000, budget() - 1] {
        assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, short)]).await, ERR_SETUP_BOUNDS);
    }
    // One lamport above the worst case for a caller cap equal to the current fee.
    assert_custom(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget() + 1)]).await, ERR_SETUP_BOUNDS);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
}

/// A caller who allows a higher fee than Raydium charges is refunded the difference,
/// and Beam still records the exact cost.
#[tokio::test]
async fn funding_for_a_higher_fee_cap_is_refunded_to_the_exact_cost() {
    let cap = CREATE_FEE + 5_000_000;
    let (mut ctx, fx) = enabled_context(Opts::standard(), cap).await;
    let payer = ctx.payer.pubkey();
    let payer_before = account(&mut ctx, &payer).await.unwrap().lamports;
    send(&mut ctx, &[migrate_ix(&fx, &payer, cap, setup_cost(cap))]).await.unwrap();
    let r = account(&mut ctx, &fx.receipt).await.expect("receipt");
    assert_eq!(u64_at(&r.data, 104 + 8 * 5), budget(), "setup_spent is the exact cost, not the funded amount");
    assert_eq!(lamports(&account(&mut ctx, &fx.authority).await), AUTHORITY_DONATION);
    let payer_after = account(&mut ctx, &payer).await.unwrap().lamports;
    // The payer pays the exact cost, the receipt's rent and a small transaction fee; the extra 5M comes back.
    let receipt_rent = rent().minimum_balance(r.data.len()) + rent().minimum_balance(209);
    let paid = payer_before - payer_after;
    assert!(paid >= budget() + receipt_rent, "payer paid less than the exact cost: {paid}");
    assert!(paid <= budget() + receipt_rent + 100_000, "refund missing: {paid} lamports left the payer");
}

/// The CPI succeeds, then a later instruction fails: pool, fee, LP and curve
/// transfers all roll back together.
#[tokio::test]
async fn failure_after_a_successful_migration_rolls_back_the_pool() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let before = snapshot(&mut ctx, &fx).await;
    let mut data = vec![2, 0, 0, 0];
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    let overdraw = Instruction { program_id: SYSTEM, accounts: vec![meta(payer, true, true), meta(Pubkey::new_unique(), true, false)], data };
    assert_failed(send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget()), overdraw]).await);
    assert_eq!(snapshot(&mut ctx, &fx).await, before);
    assert!(account(&mut ctx, &fx.ray.amm).await.is_none(), "pool must not survive");
    assert!(account(&mut ctx, &fx.receipt).await.is_none(), "receipt must not survive");
}
