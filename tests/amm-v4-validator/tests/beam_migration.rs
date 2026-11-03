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
//! Raydium runs as the pinned Rust source processor by default. Set `RAYDIUM_SO` to the SBF build
//! of the same source (`cargo build-sbf --arch v0` in raydium-amm's `program/`) and every test runs
//! against the compiled program instead; `--features live-validator` adds a run on a real
//! `solana-test-validator` (Linux). Neither is the deployed mainnet binary.
//! This does not enable graduation: production builds cannot set `enabled`.
//!
//! Run with `cargo test --locked --features beam-sbf --test beam_migration`.
#![cfg(feature = "beam-sbf")]

mod common;

use beam_amm_v4_validator::{derive_pool, expected_lp, pinned_config_bytes, process_raydium};
use common::*;
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
const UPGRADEABLE_LOADER: Pubkey = pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
const CURVE_CREATOR: Pubkey = pubkey!("Stake11111111111111111111111111111111111111");



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
/// `PumpError::BondingCurveNotComplete`, offset 6000.

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
    Mint::pack(mint, &mut data).expect("a mint packs into its fixed size");
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
    TokenAccount::pack(token, &mut data).expect("a token account packs into its fixed size");
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
fn curve_bytes(complete: bool) -> Vec<u8> {
    let zero = Pubkey::default();
    let mut d = DISC_CURVE.to_vec();
    for v in [1u64, 1, 0, REAL_QUOTE, TOTAL_SUPPLY] {
        put_u64(&mut d, v);
    }
    d.push(u8::from(complete)); // complete
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
    /// Decimals of the launch mint.
    mint_decimals: u8,
    /// Freeze the migration PDA's pre-existing base-token account.
    frozen_base: bool,
    /// Freeze the migration PDA's pre-existing wrapped-SOL account.
    frozen_wsol: bool,
    /// Whether the curve has completed its sale.
    curve_complete: bool,
    /// Lamports the curve holds below what it needs to cover rent, fee claims and the pool's quote.
    curve_short_by: u64,
    /// Tokens in the curve vault.
    vault_tokens: u64,
}

impl Opts {
    fn standard() -> Self {
        Self {
            donations: true,
            mint_owner: spl_token::id(),
            precreate_pool: false,
            production_binary: false,
            mint_decimals: DECIMALS,
            frozen_base: false,
            frozen_wsol: false,
            curve_complete: true,
            curve_short_by: 0,
            vault_tokens: VAULT_TOKENS,
        }
    }
}

/// The same token account with its state set to frozen.
fn frozen(mut account: Account) -> Account {
    let mut token = TokenAccount::unpack(&account.data).expect("token account data");
    token.state = AccountState::Frozen;
    TokenAccount::pack(token, &mut account.data).expect("a token account packs into its fixed size");
    account
}

struct Fx {
    mint: Pubkey,
    global: Pubkey,
    withdraw: Pubkey,
    curve: Pubkey,
    curve_vault: Pubkey,
    config: Pubkey,
    authority: Pubkey,
    coin_source: Pubkey,
    pc_source: Pubkey,
    lp_account: Pubkey,
    receipt: Pubkey,
    rights: Pubkey,
    ray: beam_amm_v4_validator::DerivedPool,
    donations: bool,
}

fn pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &BEAM).0
}

/// Fresh addresses for one launch; no account is created here.
fn make_fx(opts: &Opts) -> Fx {
    let mint = Pubkey::new_unique();
    let ray = derive_pool(&mint);
    let authority = pda(&[b"amm-v4-migration-authority", mint.as_ref()]);
    let curve = pda(&[b"bonding-curve", mint.as_ref()]);
    Fx {
        mint,
        global: pda(&[b"global"]),
        withdraw: Pubkey::new_unique(),
        curve,
        curve_vault: spl_associated_token_account::get_associated_token_address(&curve, &mint),
        config: pda(&[b"beam-amm-v4-config"]),
        authority,
        coin_source: spl_associated_token_account::get_associated_token_address(&authority, &mint),
        pc_source: spl_associated_token_account::get_associated_token_address(&authority, &spl_token::native_mint::id()),
        lp_account: spl_associated_token_account::get_associated_token_address(&authority, &ray.lp_mint),
        receipt: pda(&[b"beam-amm-v4-migration", mint.as_ref()]),
        rights: pda(&[b"beam-amm-rights", ray.amm.as_ref()]),
        ray,
        donations: opts.donations,
    }
}

fn build(opts: Opts) -> (ProgramTest, Fx) {
    let fx = make_fx(&opts);
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
    // By default Raydium runs as its Rust source processor. With RAYDIUM_SO set to the SBF build of
    // the same source, every test here runs against the compiled program instead (see the header).
    match std::env::var("RAYDIUM_SO") {
        Ok(path) => {
            // Loaded the way mainnet runs it: an upgradeable-loader program account that points at a
            // program-data account holding a 45-byte header (tag 3, slot, authority option) and the ELF.
            let elf = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read RAYDIUM_SO {path}: {e}"));
            let programdata = Pubkey::find_program_address(&[raydium_amm::id().as_ref()], &UPGRADEABLE_LOADER).0;
            let mut program_state = 2u32.to_le_bytes().to_vec();
            program_state.extend_from_slice(programdata.as_ref());
            let mut data_state = 3u32.to_le_bytes().to_vec();
            data_state.extend_from_slice(&0u64.to_le_bytes());
            data_state.resize(45, 0);
            data_state.extend_from_slice(&elf);
            pt.add_account(
                raydium_amm::id(),
                Account { lamports: rent().minimum_balance(program_state.len()), data: program_state, owner: UPGRADEABLE_LOADER, executable: true, rent_epoch: 0 },
            );
            pt.add_account(
                programdata,
                Account { lamports: rent().minimum_balance(data_state.len()), data: data_state, owner: UPGRADEABLE_LOADER, executable: false, rent_epoch: 0 },
            );
        }
        Err(_) => pt.add_program("raydium_amm", raydium_amm::id(), processor!(process_raydium)),
    }

    for (key, account) in fixture_accounts(&opts, &fx) {
        pt.add_account(key, account);
    }
    (pt, fx)
}

/// Every account the fixtures preload except Global, which names the payer as authority and
/// so is written once the payer exists. Shared by the program-test bank and the live validator.
fn fixture_accounts(opts: &Opts, fx: &Fx) -> Vec<(Pubkey, Account)> {
    let (mint, authority, curve) = (fx.mint, fx.authority, fx.curve);
    let sys = |lamports: u64| Account { lamports, data: vec![], owner: SYSTEM, executable: false, rent_epoch: 0 };
    let program_owned = |data: Vec<u8>| Account { lamports: rent().minimum_balance(data.len()), data, owner: BEAM, executable: false, rent_epoch: 0 };

    let curve_data = curve_bytes(opts.curve_complete);
    let curve_lamports = rent().minimum_balance(curve_data.len()) + CREATOR_FEE + PROTOCOL_FEES + REAL_QUOTE + POST_COMPLETE_QUOTE + CURVE_SURPLUS - opts.curve_short_by;
    let mut out = vec![
        (fx.curve, Account { lamports: curve_lamports, ..program_owned(curve_data) }),
        (fx.mint, mint_account(&opts.mint_owner, opts.mint_decimals, TOTAL_SUPPLY)),
        (fx.curve_vault, token_account(&mint, &curve, opts.vault_tokens, None)),
        (spl_token::native_mint::id(), mint_account(&spl_token::id(), 9, 0)),
    ];
    // Raydium's canonical config, with the pool-creation fee the processor charges.
    let cfg = pinned_config_bytes(&raydium_amm::processor::config_feature::amm_owner::id(), &raydium_amm::processor::config_feature::amm_owner::id(), CREATE_FEE);
    out.push((fx.ray.config, Account { lamports: rent().minimum_balance(cfg.len()), data: cfg, owner: raydium_amm::id(), executable: false, rent_epoch: 0 }));
    out.push((
        fx.ray.fee_destination,
        token_account(&spl_token::native_mint::id(), &Pubkey::new_unique(), FEE_DEST_DONATION, Some(rent().minimum_balance(TokenAccount::LEN))),
    ));
    if opts.donations {
        out.push((fx.authority, sys(AUTHORITY_DONATION)));
        let base = token_account(&mint, &authority, BASE_DONATION, None);
        let wsol = token_account(&spl_token::native_mint::id(), &authority, WSOL_DONATION, Some(rent().minimum_balance(TokenAccount::LEN)));
        out.push((fx.coin_source, if opts.frozen_base { frozen(base) } else { base }));
        out.push((fx.pc_source, if opts.frozen_wsol { frozen(wsol) } else { wsol }));
    }
    if opts.precreate_pool {
        out.push((fx.ray.amm, sys(1)));
    }
    out
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
    migrate_ix_at(fx, user, 0, max_fee, budget)
}

/// `migrate_amm_v4(open_time, max_create_pool_fee, setup_budget)`.
fn migrate_ix_at(fx: &Fx, user: &Pubkey, open_time: u64, max_fee: u64, budget: u64) -> Instruction {
    let mut data = DISC_MIGRATE.to_vec();
    for v in [open_time, max_fee, budget] {
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
        meta(fx.coin_source, true, false),
        meta(wsol, false, false),
        meta(fx.pc_source, true, false),
        meta(fx.ray.amm, true, false),
        meta(fx.ray.amm_authority, false, false),
        meta(fx.ray.lp_mint, true, false),
        meta(fx.ray.coin_vault, true, false),
        meta(fx.ray.pc_vault, true, false),
        meta(fx.ray.target_orders, true, false),
        meta(fx.ray.fee_destination, true, false),
        meta(fx.lp_account, true, false),
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
    let blockhash = ctx.get_new_latest_blockhash().await.expect("the bank provides a blockhash");
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

/// Reads accounts from the program-test bank or from a live validator, so the effect
/// assertions below run unchanged against either.
trait Reader {
    async fn read(&mut self, key: &Pubkey) -> Option<Account>;
}

impl Reader for ProgramTestContext {
    async fn read(&mut self, key: &Pubkey) -> Option<Account> {
        self.banks_client.get_account(*key).await.expect("the bank answers account reads")
    }
}

async fn account<R: Reader>(ctx: &mut R, key: &Pubkey) -> Option<Account> {
    ctx.read(key).await
}

/// Reads an account that the test requires to exist, naming it when it does not.
async fn must<R: Reader>(ctx: &mut R, key: &Pubkey) -> Account {
    account(ctx, key).await.unwrap_or_else(|| panic!("account {key} must exist"))
}

fn amount(a: &Account) -> u64 {
    TokenAccount::unpack(&a.data).expect("token account data").amount
}

/// Every account a migration could touch, for exact rollback comparison.
async fn snapshot<R: Reader>(ctx: &mut R, fx: &Fx) -> Vec<Option<Account>> {
    let keys = [
        fx.curve, fx.curve_vault, fx.withdraw, fx.authority, fx.coin_source, fx.pc_source, fx.lp_account, fx.receipt, fx.rights, fx.config,
        fx.ray.amm, fx.ray.lp_mint, fx.ray.coin_vault, fx.ray.pc_vault, fx.ray.target_orders, fx.ray.fee_destination, fx.mint,
    ];
    let mut out = Vec::new();
    for key in keys {
        out.push(account(ctx, &key).await);
    }
    out
}

/// Sends `ixs`, expects the custom error `code` (any failure when `None`), and checks that no account moved.
async fn refuses(ctx: &mut ProgramTestContext, fx: &Fx, ixs: &[Instruction], code: Option<u32>, what: &str) {
    let before = snapshot(ctx, fx).await;
    let result = send(ctx, ixs).await;
    match code {
        Some(code) => assert_custom(result, code),
        None => assert_failed(result),
    }
    assert_eq!(snapshot(ctx, fx).await, before, "{what} must change nothing");
}

/// `refuses` for the standard migration call: the test payer, the stored fee cap, the exact setup budget.
async fn refuses_standard(ctx: &mut ProgramTestContext, fx: &Fx, code: Option<u32>, what: &str) {
    let ix = migrate_ix(fx, &ctx.payer.pubkey(), CREATE_FEE, budget());
    refuses(ctx, fx, &[ix], code, what).await;
}

async fn enabled_context(opts: Opts, cap: u64) -> (ProgramTestContext, Fx) {
    let (pt, fx) = build(opts);
    let mut ctx = start(pt, &fx).await;
    let payer = ctx.payer.pubkey();
    send(&mut ctx, &[configure_ix(&fx, &payer, cap)]).await.expect("the transaction succeeds");
    send(&mut ctx, &[enable_ix(&fx, &payer, true)]).await.expect("the transaction succeeds");
    (ctx, fx)
}


fn lamports(a: &Option<Account>) -> u64 {
    a.as_ref().map_or(0, |a| a.lamports)
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().expect("eight bytes at the offset"))
}

const TARGET_ORDERS_LEN: usize = 2208;
const AMM_INFO_LEN: usize = 752;

/// A migrated pool must be readable by a graduated-ancestor quote: SwapOnly status,
/// no pending PnL, identity fields equal to the addresses Beam derives (the mint is the
/// market seed), vaults owned by Raydium's authority with no delegate or close authority,
/// and a WSOL vault fully backed by lamports. These are the checks the AMM v4 quote adapter
/// in draft PR #49 makes on raw pool bytes; the offsets are pinned in `pool_layout.rs`.
/// The rights record it also needs is written by the migration; see `assert_rights_record`.
async fn assert_quote_ready<R: Reader>(ctx: &mut R, fx: &Fx, coin: u64, pc: u64) {
    let pool = must(ctx, &fx.ray.amm).await.data;
    let key_at = |at: usize| Pubkey::try_from(&pool[at..at + 32]).expect("a 32-byte key at the offset");
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
        let acct = must(ctx, &vault).await;
        assert_eq!(acct.owner, spl_token::id());
        let t = TokenAccount::unpack(&acct.data).expect("token account data");
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
async fn assert_rights_record<R: Reader>(ctx: &mut R, fx: &Fx) {
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
    send(ctx, &[migrate_ix(fx, &payer, CREATE_FEE, budget())]).await.expect("the transaction succeeds");
    assert_migrated(ctx, fx, &before).await;
}

/// Every effect of a successful standard migration, read from any backend, against the
/// accounts as they were just before it.
async fn assert_migrated<R: Reader>(ctx: &mut R, fx: &Fx, before: &[Option<Account>]) {
    let pc = REAL_QUOTE + POST_COMPLETE_QUOTE - MIGRATION_FEE;
    let pool = account(ctx, &fx.ray.amm).await.expect("pool created");
    assert_eq!(pool.owner, raydium_amm::id());
    assert_eq!(pool.data.len(), AMM_INFO_LEN);
    assert_eq!(u64_at(&pool.data, 0), 6, "SwapOnly");
    assert_eq!(amount(&must(ctx, &fx.ray.coin_vault).await), VAULT_TOKENS);
    assert_eq!(amount(&must(ctx, &fx.ray.pc_vault).await), pc);
    assert_quote_ready(ctx, fx, VAULT_TOKENS, pc).await;

    // Raydium issues LP to the migration PDA; Beam burns that entire issuance.
    let (_, user_lp) = expected_lp(VAULT_TOKENS, pc, DECIMALS).expect("the planned pool issues LP");
    let lp_mint = Mint::unpack(&must(ctx, &fx.ray.lp_mint).await.data).expect("LP mint data");
    assert_eq!(lp_mint.supply, 0, "the minted LP supply was burned");
    let lp = TokenAccount::unpack(&must(ctx, &fx.lp_account).await.data).expect("LP account data");
    assert_eq!((lp.owner, lp.mint, lp.amount), (fx.authority, fx.ray.lp_mint, 0));

    // Raydium's fee lands once; the curve keeps rent, fee claims and the surplus.
    assert_eq!(amount(&must(ctx, &fx.ray.fee_destination).await), FEE_DEST_DONATION + CREATE_FEE);
    assert_eq!(amount(&must(ctx, &fx.curve_vault).await), 0);
    let curve = must(ctx, &fx.curve).await;
    assert_eq!(curve.lamports, lamports(&before[0]) - (REAL_QUOTE + POST_COMPLETE_QUOTE));
    assert_eq!(curve.lamports, rent().minimum_balance(curve.data.len()) + CREATOR_FEE + PROTOCOL_FEES + CURVE_SURPLUS);
    assert_eq!(lamports(&account(ctx, &fx.withdraw).await), MIGRATION_FEE);

    // Unrelated donations are neither adopted nor swept, and unspent setup is refunded.
    let (base_d, wsol_d, auth_d) = if fx.donations { (BASE_DONATION, WSOL_DONATION, AUTHORITY_DONATION) } else { (0, 0, 0) };
    assert_eq!(amount(&must(ctx, &fx.coin_source).await), base_d);
    let wsol = must(ctx, &fx.pc_source).await;
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
    assert_eq!(r.data[160], 1, "Beam burned all LP it received");
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
    send(&mut ctx, &[configure_ix(&fx, &payer, CREATE_FEE)]).await.expect("the transaction succeeds");
    refuses_standard(&mut ctx, &fx, Some(ERR_NOT_ENABLED), "a disabled config").await;
}

/// The binary a release would use has no `experimental-amm-v4`: the authority cannot
/// switch migration on, so `migrate_amm_v4` stays unreachable even with every account valid.
#[tokio::test]
async fn production_build_cannot_enable_migration() {
    let (pt, fx) = build(Opts { production_binary: true, ..Opts::standard() });
    let mut ctx = start(pt, &fx).await;
    let payer = ctx.payer.pubkey();
    send(&mut ctx, &[configure_ix(&fx, &payer, CREATE_FEE)]).await.expect("the transaction succeeds");
    refuses(&mut ctx, &fx, &[enable_ix(&fx, &payer, true)], Some(ERR_NOT_ENABLED), "a refused enable").await;
    refuses_standard(&mut ctx, &fx, Some(ERR_NOT_ENABLED), "a migration that was never enabled").await;
    // Turning it off is always allowed.
    send(&mut ctx, &[enable_ix(&fx, &payer, false)]).await.expect("the transaction succeeds");
}

fn compute_limit(units: u32) -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&units.to_le_bytes());
    Instruction { program_id: pubkey!("ComputeBudget111111111111111111111111111111"), accounts: vec![], data }
}

#[tokio::test]
async fn an_incomplete_curve_is_refused_before_anything_moves() {
    let (mut ctx, fx) = enabled_context(Opts { curve_complete: false, ..Opts::standard() }, CREATE_FEE).await;
    refuses_standard(&mut ctx, &fx, Some(ERR_CURVE_NOT_COMPLETE), "an incomplete curve").await;
}

/// The curve must cover its rent, the creator and protocol fee claims, and the pool's quote. One
/// lamport short is refused; exactly covering it migrates and leaves the curve at that floor.
#[tokio::test]
async fn a_curve_one_lamport_short_of_its_liabilities_is_refused_and_an_exact_one_migrates() {
    let (mut ctx, fx) = enabled_context(Opts { curve_short_by: CURVE_SURPLUS + 1, ..Opts::standard() }, CREATE_FEE).await;
    refuses_standard(&mut ctx, &fx, Some(ERR_BALANCE), "a short curve").await;

    let (mut ctx, fx) = enabled_context(Opts { curve_short_by: CURVE_SURPLUS, ..Opts::standard() }, CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    send(&mut ctx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget())]).await.expect("an exactly covered curve migrates");
    let curve = must(&mut ctx, &fx.curve).await;
    assert_eq!(curve.lamports, rent().minimum_balance(curve.data.len()) + CREATOR_FEE + PROTOCOL_FEES, "the curve keeps exactly its protected floor");
}

#[tokio::test]
async fn an_empty_or_oversized_curve_vault_is_refused() {
    for vault_tokens in [0, TOTAL_SUPPLY + 1] {
        let (mut ctx, fx) = enabled_context(Opts { vault_tokens, ..Opts::standard() }, CREATE_FEE).await;
        refuses_standard(&mut ctx, &fx, Some(ERR_BALANCE), &format!("{vault_tokens} vault tokens")).await;
    }
}

/// Raydium's initializer overflows a `u64` for a mint with 19 or more decimals, so Beam refuses
/// such a mint with a clear error before any funds move.
#[tokio::test]
async fn mint_with_too_many_decimals_is_refused_before_anything_moves() {
    for decimals in [19u8, 255] {
        let (mut ctx, fx) = enabled_context(Opts { mint_decimals: decimals, ..Opts::standard() }, CREATE_FEE).await;
        refuses_standard(&mut ctx, &fx, Some(ERR_UNSUPPORTED_MINT), &format!("{decimals} decimals")).await;
    }
}

/// A frozen source account would make Raydium's transfer fail mid-CPI; it is refused up front.
#[tokio::test]
async fn frozen_source_accounts_are_refused_before_anything_moves() {
    for opts in [Opts { frozen_base: true, ..Opts::standard() }, Opts { frozen_wsol: true, ..Opts::standard() }] {
        let (mut ctx, fx) = enabled_context(opts, CREATE_FEE).await;
        refuses_standard(&mut ctx, &fx, Some(ERR_SOURCE), "a frozen source").await;
    }
}

/// The 19-account `Initialize2` call exactly as Beam makes it, but for an outside wallet.
fn raydium_initialize2_ix(fx: &Fx, wallet: &Pubkey, coin_source: &Pubkey, pc_source: &Pubkey, init_pc: u64, init_coin: u64) -> Instruction {
    let mut data = vec![1u8, fx.ray.nonce];
    for v in [0u64, init_pc, init_coin] {
        put_u64(&mut data, v);
    }
    let accounts = vec![
        meta(spl_token::id(), false, false),
        meta(spl_associated_token_account::id(), false, false),
        meta(SYSTEM, false, false),
        meta(solana_program::sysvar::rent::ID, false, false),
        meta(fx.ray.amm, true, false),
        meta(fx.ray.amm_authority, false, false),
        meta(fx.ray.lp_mint, true, false),
        meta(fx.mint, false, false),
        meta(spl_token::native_mint::id(), false, false),
        meta(fx.ray.coin_vault, true, false),
        meta(fx.ray.pc_vault, true, false),
        meta(fx.ray.target_orders, true, false),
        meta(fx.ray.config, false, false),
        meta(fx.ray.fee_destination, true, false),
        meta(fx.mint, false, false), // the pool seed: the launch mint, which anyone can compute
        meta(*wallet, true, true),
        meta(*coin_source, true, false),
        meta(*pc_source, true, false),
        meta(spl_associated_token_account::get_associated_token_address(wallet, &fx.ray.lp_mint), true, false),
    ];
    Instruction { program_id: raydium_amm::id(), accounts, data }
}

/// Pool-seed pre-emption, demonstrated for real rather than argued: because the pool address is
/// derived from the public mint, anyone holding a little of the token and some SOL can create the
/// pool through Raydium first, at a price of their choosing. Beam then refuses to migrate and
/// moves nothing. The harm is availability (this curve can no longer graduate through this
/// instruction), not loss of funds. See docs/AMM_V4_MIGRATION.md, "Pool-seed pre-emption".
#[tokio::test]
async fn a_third_party_can_create_the_pool_first_and_beam_then_fails_closed() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let outsider = ctx.payer.pubkey();
    let (coin_source, pc_source) = (Pubkey::new_unique(), Pubkey::new_unique());
    let tokens = 1_000_000_000u64;
    let reserve = rent().minimum_balance(TokenAccount::LEN);
    ctx.set_account(&coin_source, &token_account(&fx.mint, &outsider, tokens, None).into());
    ctx.set_account(&pc_source, &token_account(&spl_token::native_mint::id(), &outsider, tokens, Some(reserve)).into());

    // The outsider's own transaction succeeds: nothing in Raydium ties the pool to Beam.
    send(&mut ctx, &[raydium_initialize2_ix(&fx, &outsider, &coin_source, &pc_source, tokens, tokens)])
        .await
        .expect("an outsider can create the pool at the mint-derived address");
    let pool = must(&mut ctx, &fx.ray.amm).await;
    assert_eq!(pool.owner, raydium_amm::id());
    assert_eq!(u64_at(&pool.data, 0), 6, "the outsider's pool is already trading");

    // Beam's migration now fails closed and leaves the curve, its tokens and every balance alone.
    refuses_standard(&mut ctx, &fx, Some(ERR_INVALID_ACCOUNT), "a pre-empted migration").await;
    assert!(account(&mut ctx, &fx.receipt).await.is_none(), "no receipt is written for a pre-empted migration");
}

/// A caller must not be able to schedule the pool to open later: Raydium would create it
/// waiting, nobody could trade, and the migration could not be redone.
#[tokio::test]
async fn pool_cannot_be_scheduled_to_open_in_the_future() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    for open_time in [4_102_444_800u64, u64::MAX] {
        let ix = migrate_ix_at(&fx, &payer, open_time, CREATE_FEE, budget());
        refuses(&mut ctx, &fx, &[ix], Some(ERR_OPEN_TIME), "a future open time").await;
    }
    // The same migration with the open time left at zero still works.
    assert_migrates(&mut ctx, &fx).await;
}

#[tokio::test]
async fn replay_is_rejected_and_leaves_the_pool_untouched() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    assert_migrates(&mut ctx, &fx).await;
    refuses_standard(&mut ctx, &fx, None, "a replay").await;
}

#[tokio::test]
async fn create_pool_fee_must_fit_both_the_caller_and_the_stored_cap() {
    // Caller cap above the stored cap.
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    refuses(&mut ctx, &fx, &[migrate_ix(&fx, &payer, CREATE_FEE + 1, budget())], Some(ERR_FEE_CAP), "a cap above the stored cap").await;
    // Raydium's current fee above a caller cap that is within the stored cap.
    refuses(&mut ctx, &fx, &[migrate_ix(&fx, &payer, CREATE_FEE - 1, budget())], Some(ERR_FEE_CAP), "a cap below Raydium's fee").await;
}

#[tokio::test]
async fn token_2022_mint_fails_before_liquidity_moves() {
    let token_2022 = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
    let (mut ctx, fx) = enabled_context(Opts { mint_owner: token_2022, ..Opts::standard() }, CREATE_FEE).await;
    refuses_standard(&mut ctx, &fx, None, "a Token-2022 mint").await;
}

/// A funded account at the deterministic pool address blocks migration safely.
/// It also shows the cost of a mint-derived AMM seed: anyone who creates that
/// pool first stops this launch from graduating. See docs/AMM_V4_INTEGRATION.md.
#[tokio::test]
async fn precreated_pool_blocks_migration_without_moving_liquidity() {
    let (mut ctx, fx) = enabled_context(Opts { precreate_pool: true, ..Opts::standard() }, CREATE_FEE).await;
    refuses_standard(&mut ctx, &fx, Some(ERR_INVALID_ACCOUNT), "a pre-created pool").await;
}

#[tokio::test]
async fn substituted_accounts_are_rejected() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
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
        (21, "receipt", false),
        (22, "rights", false),
    ] {
        let mut ix = migrate_ix(&fx, &payer, CREATE_FEE, budget());
        ix.accounts[index].pubkey = Pubkey::new_unique();
        // The rights record's address is derived from the pool, so Anchor's seeds check
        // (ConstraintSeeds, 2006) rejects a substituted pool before Beam's own check runs.
        let code = match (name, beam_error) {
            ("pool", _) => Some(2006),
            (_, true) => Some(ERR_INVALID_ACCOUNT),
            (_, false) => None,
        };
        refuses(&mut ctx, &fx, &[ix], code, &format!("substituting {name}")).await;
    }
}

/// Setup funding must cover the exact cost and may not exceed the worst case the
/// caller accepted. Both sides fail before any lamport or token moves.
#[tokio::test]
async fn setup_funding_must_sit_between_the_exact_cost_and_the_accepted_worst_case() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    // The create fee alone, one lamport short of the real cost, and one lamport above the worst case
    // for a caller cap equal to the current fee.
    for funded in [CREATE_FEE + 1_000, budget() - 1, budget() + 1] {
        refuses(&mut ctx, &fx, &[migrate_ix(&fx, &payer, CREATE_FEE, funded)], Some(ERR_SETUP_BOUNDS), &format!("{funded} lamports of setup")).await;
    }
}

/// A caller who allows a higher fee than Raydium charges is refunded the difference,
/// and Beam still records the exact cost.
#[tokio::test]
async fn funding_for_a_higher_fee_cap_is_refunded_to_the_exact_cost() {
    let cap = CREATE_FEE + 5_000_000;
    let (mut ctx, fx) = enabled_context(Opts::standard(), cap).await;
    let payer = ctx.payer.pubkey();
    let payer_before = must(&mut ctx, &payer).await.lamports;
    send(&mut ctx, &[migrate_ix(&fx, &payer, cap, setup_cost(cap))]).await.expect("the transaction succeeds");
    let r = account(&mut ctx, &fx.receipt).await.expect("receipt");
    assert_eq!(u64_at(&r.data, 104 + 8 * 5), budget(), "setup_spent is the exact cost, not the funded amount");
    assert_eq!(lamports(&account(&mut ctx, &fx.authority).await), AUTHORITY_DONATION);
    let payer_after = must(&mut ctx, &payer).await.lamports;
    // The payer pays the exact cost, the receipt's rent and a small transaction fee; the extra 5M comes back.
    let receipt_rent = rent().minimum_balance(r.data.len()) + rent().minimum_balance(209);
    let paid = payer_before - payer_after;
    assert!(paid >= budget() + receipt_rent, "payer paid less than the exact cost: {paid}");
    assert!(paid <= budget() + receipt_rent + 100_000, "refund missing: {paid} lamports left the payer");
}

/// The CPI and LP burn succeed, then a later instruction fails: pool, fee, LP
/// issuance and burn, and curve transfers all roll back together.
#[tokio::test]
async fn failure_after_a_successful_migration_rolls_back_the_pool() {
    let (mut ctx, fx) = enabled_context(Opts::standard(), CREATE_FEE).await;
    let payer = ctx.payer.pubkey();
    let mut data = vec![2, 0, 0, 0];
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    let overdraw = Instruction { program_id: SYSTEM, accounts: vec![meta(payer, true, true), meta(Pubkey::new_unique(), true, false)], data };
    refuses(&mut ctx, &fx, &[migrate_ix(&fx, &payer, CREATE_FEE, budget()), overdraw], None, "a failure after a successful migration").await;
    assert!(account(&mut ctx, &fx.ray.amm).await.is_none(), "pool must not survive");
    assert!(account(&mut ctx, &fx.receipt).await.is_none(), "receipt must not survive");
}

/// The same migration on a real `solana-test-validator`: Beam and the pinned Raydium program
/// both run as compiled SBF in the validator's own runtime, the fixtures are genesis accounts,
/// and transactions go over RPC. Anza ships the validator for Linux only, so this runs in CI
/// (`.github/workflows/beam-amm-v4-sbf.yml`), not on Windows.
///
/// Needs `solana-test-validator` on PATH and `RAYDIUM_SO`, the SBF build of raydium-amm at the
/// pin (`cargo build-sbf --arch v0` in its `program/` directory). `BEAM_SO` defaults to the
/// local-test Beam build. Run with `--features live-validator`.
#[cfg(feature = "live-validator")]
mod live {
    use super::*;
    use solana_commitment_config::CommitmentConfig;
    use solana_keypair::Keypair;
    use solana_rpc_client::nonblocking::rpc_client::RpcClient;
    use std::{
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    impl Reader for RpcClient {
        async fn read(&mut self, key: &Pubkey) -> Option<Account> {
            self.get_account_with_commitment(key, CommitmentConfig::confirmed()).await.expect("the validator answers account reads").value
        }
    }

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    /// The JSON the validator's `--account` flag reads (the shape of `solana account -o json`).
    fn write_account(dir: &Path, key: &Pubkey, a: &Account) -> PathBuf {
        let path = dir.join(format!("{key}.json"));
        let json = format!(
            r#"{{"pubkey":"{key}","account":{{"lamports":{},"data":["{}","base64"],"owner":"{}","executable":{},"rentEpoch":0,"space":{}}}}}"#,
            a.lamports,
            b64(&a.data),
            a.owner,
            a.executable,
            a.data.len()
        );
        std::fs::write(&path, json).expect("the account file is written");
        path
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").expect("a free local port").local_addr().expect("a local address").port()
    }

    /// Kills the validator and removes its ledger even when an assertion panics.
    struct Validator {
        child: Child,
        dir: PathBuf,
    }

    impl Drop for Validator {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    async fn signed_tx(rpc: &RpcClient, payer: &Keypair, ixs: &[Instruction], units: u32) -> Result<Transaction, String> {
        let blockhash = rpc.get_latest_blockhash().await.map_err(|e| format!("{e:?}"))?;
        let mut all = vec![compute_limit(units)];
        all.extend_from_slice(ixs);
        Ok(Transaction::new_signed_with_payer(&all, Some(&payer.pubkey()), &[payer], blockhash))
    }

    async fn send_tx(rpc: &RpcClient, payer: &Keypair, ixs: &[Instruction]) -> Result<(), String> {
        send_with_limit(rpc, payer, ixs, 1_400_000).await
    }

    async fn send_with_limit(rpc: &RpcClient, payer: &Keypair, ixs: &[Instruction], units: u32) -> Result<(), String> {
        let tx = signed_tx(rpc, payer, ixs, units).await?;
        rpc.send_and_confirm_transaction(&tx).await.map(|_| ()).map_err(|e| format!("{e:?}"))
    }

    /// Simulates the transaction and returns the compute units it uses and its serialized size.
    async fn measure(rpc: &RpcClient, payer: &Keypair, ixs: &[Instruction]) -> (u64, usize) {
        let tx = signed_tx(rpc, payer, ixs, 1_400_000).await.expect("a signed transaction");
        // One-byte signature count, then 64 bytes per signature, then the message.
        let bytes = tx.message.serialize().len() + 1 + 64 * tx.signatures.len();
        let sim = rpc.simulate_transaction(&tx).await.expect("the validator simulates the transaction").value;
        assert!(sim.err.is_none(), "simulation failed: {:?} {:?}", sim.err, sim.logs);
        (sim.units_consumed.expect("the simulation reports compute units"), bytes)
    }

    #[tokio::test]
    async fn live_validator_migrates_through_real_sbf_programs() {
        let beam_so = std::env::var("BEAM_SO")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../programs/beam/target/deploy/beam_program.so").to_string());
        let raydium_so = std::env::var("RAYDIUM_SO").expect("set RAYDIUM_SO to the SBF build of raydium-amm at the pin");
        for so in [&beam_so, &raydium_so] {
            assert!(Path::new(so).exists(), "missing program binary: {so}");
        }

        let payer = Keypair::new();
        let fx = make_fx(&Opts::standard());
        let dir = std::env::temp_dir().join(format!("beam-live-{}", std::process::id()));
        let accounts_dir = dir.join("accounts");
        std::fs::create_dir_all(&accounts_dir).expect("the accounts directory is created");

        // Global names the payer as authority, so it is written here with the other fixtures.
        let global = global_bytes(&payer.pubkey(), &fx.withdraw);
        let mut accounts = fixture_accounts(&Opts::standard(), &fx);
        accounts.push((fx.global, Account { lamports: rent().minimum_balance(global.len()), data: global, owner: BEAM, executable: false, rent_epoch: 0 }));

        let (rpc_port, faucet_port) = (free_port(), free_port());
        let mut cmd = Command::new("solana-test-validator");
        cmd.args(["--reset", "--quiet", "--ledger"])
            .arg(dir.join("ledger"))
            .args(["--rpc-port", &rpc_port.to_string(), "--faucet-port", &faucet_port.to_string()])
            .args(["--bpf-program", &BEAM.to_string(), &beam_so])
            // Raydium is upgradeable on mainnet, so load it under the upgradeable loader here too.
            .args(["--upgradeable-program", &raydium_amm::id().to_string(), &raydium_so, "none"]);
        for (key, account) in &accounts {
            let file = write_account(&accounts_dir, key, account);
            cmd.arg("--account").arg(key.to_string()).arg(file);
        }
        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("solana-test-validator must be on PATH (Linux release from anza-xyz/agave)");
        let mut validator = Validator { child, dir };

        let mut rpc = RpcClient::new_with_commitment(format!("http://127.0.0.1:{rpc_port}"), CommitmentConfig::confirmed());
        let started = Instant::now();
        loop {
            if let Some(status) = validator.child.try_wait().expect("the validator process can be polled") {
                panic!("solana-test-validator exited early: {status}");
            }
            if rpc.get_health().await.is_ok() {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(180), "validator did not become healthy in 180s");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let signature = rpc.request_airdrop(&payer.pubkey(), 20_000_000_000).await.expect("the faucet funds the payer");
        while !rpc.confirm_transaction(&signature).await.expect("the airdrop can be confirmed") {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        let key = payer.pubkey();
        send_tx(&rpc, &payer, &[configure_ix(&fx, &key, CREATE_FEE)]).await.expect("the transaction succeeds");
        send_tx(&rpc, &payer, &[enable_ix(&fx, &key, true)]).await.expect("the transaction succeeds");

        let before = snapshot(&mut rpc, &fx).await;
        // Measured on the live validator: a client must request this much compute, and the
        // transaction must fit the 1232-byte packet limit.
        let (units, bytes) = measure(&rpc, &payer, &[migrate_ix(&fx, &key, CREATE_FEE, budget())]).await;
        println!("MEASURED migrate_amm_v4: {units} compute units, {bytes} byte transaction");
        assert!(units > 200_000, "the default 200000 limit must not suffice, or the docs are wrong: {units}");
        assert!(units < 1_400_000 && bytes <= 1232, "{units} units, {bytes} bytes");
        // The validator enforces a transaction's own compute limit (the program-test bank does not), so
        // this is where "too little compute" is meaningful: it must fail and change nothing.
        let starved = send_with_limit(&rpc, &payer, &[migrate_ix(&fx, &key, CREATE_FEE, budget())], 100_000).await;
        assert!(starved.is_err(), "100000 compute units must not be enough for a migration");
        assert_eq!(snapshot(&mut rpc, &fx).await, before, "running out of compute must change nothing");
        send_tx(&rpc, &payer, &[migrate_ix(&fx, &key, CREATE_FEE, budget())]).await.expect("migration on the live validator");
        assert_migrated(&mut rpc, &fx, &before).await;

        // The pool exists now, so a replay must fail and leave everything as it is.
        let after = snapshot(&mut rpc, &fx).await;
        assert!(send_tx(&rpc, &payer, &[migrate_ix(&fx, &key, CREATE_FEE, budget())]).await.is_err(), "replay must fail");
        assert_eq!(snapshot(&mut rpc, &fx).await, after, "a failed replay changed state");
    }
}
