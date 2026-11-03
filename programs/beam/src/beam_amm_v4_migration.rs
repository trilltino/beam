//! Gated Raydium AMM v4 migration: production builds cannot enable it, every Raydium address is derived from the mint, and all LP Beam receives is burned.
#![allow(clippy::needless_pass_by_value)]
use crate::{
    beam_amm_v4 as abi,
    beam_amm_v4_pool_state::{AmmInfo, AMM_INFO_LEN, TARGET_ORDERS_LEN},
    beam_migration_math::{migration_quote, protected_curve_floor},
    beam_policy::BeamError,
    trade_types::BondingCurve,
    Global, PumpError,
};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
    program_pack::Pack,
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Burn, Mint, Token, TokenAccount, TransferChecked},
};

pub const V4_CONFIG_SEED: &[u8] = b"beam-amm-v4-config";
pub const V4_AUTH_SEED: &[u8] = b"amm-v4-migration-authority";
pub const V4_RECEIPT_SEED: &[u8] = b"beam-amm-v4-migration";
pub const V4_RIGHTS_SEED: &[u8] = b"beam-amm-rights";
pub const V4_RIGHTS_SPACE: usize = 8 + 6 * 32 + 8 + 1;
pub const V4_MAX_DECIMALS: u8 = 18;
const MARKET_SEED_SLOT: usize = 14;
/// [`BeamAmmV4Receipt::lp_disposition`]: all LP issued to Beam was burned.
pub const LP_BURNED: u8 = 1;

#[cfg(feature = "raydium-devnet")]
pub const AMM_V4_PROGRAM: Pubkey = abi::DEVNET_PROGRAM_ID;
#[cfg(not(feature = "raydium-devnet"))]
pub const AMM_V4_PROGRAM: Pubkey = abi::MAINNET_PROGRAM_ID;

#[account]
pub struct BeamAmmV4Config {
    pub enabled: bool,
    pub raydium_program: Pubkey,
    pub max_pool_fee: u64,
}

#[account(discriminator = [88, 155, 79, 159, 221, 232, 159, 98])]
pub struct BeamAmmV4Rights {
    pub pool: Pubkey,
    pub mint: Pubkey,
    pub market_seed: Pubkey,
    pub creator: Pubkey,
    pub creator_payout: Pubkey,
    pub donation_record: Pubkey,
    pub revision: u64,
    pub paused: bool,
}

#[account]
pub struct BeamAmmV4Receipt {
    pub mint: Pubkey,
    pub pool: Pubkey,
    pub lp_mint: Pubkey,
    pub coin_amount: u64,
    pub pc_amount: u64,
    pub migration_fee: u64,
    pub create_pool_fee: u64,
    pub lp_amount: u64,
    pub setup_spent: u64,
    pub open_time: u64,
    pub lp_disposition: u8,
}

#[event]
pub struct BeamAmmV4Migration {
    pub mint: Pubkey,
    pub pool: Pubkey,
    pub coin_amount: u64,
    pub pc_amount: u64,
    pub migration_fee: u64,
    pub create_pool_fee: u64,
    pub lp_amount: u64,
}

#[derive(Accounts)]
pub struct ConfigureV4<'info> {
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(init, payer = authority, space = 8 + 1 + 32 + 8, seeds = [V4_CONFIG_SEED], bump)]
    pub config: Account<'info, BeamAmmV4Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct EnableV4<'info> {
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    pub authority: Signer<'info>,
    #[account(mut, seeds = [V4_CONFIG_SEED], bump)]
    pub config: Account<'info, BeamAmmV4Config>,
}

#[derive(Accounts)]
pub struct MigrateV4<'info> {
    #[account(seeds = [b"global"], bump, has_one = withdraw_authority)]
    pub global: Box<Account<'info, Global>>,
    /// CHECK: constrained to the recipient configured in Global.
    #[account(mut)]
    pub withdraw_authority: UncheckedAccount<'info>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, seeds = [b"bonding-curve", mint.key().as_ref()], bump)]
    pub bonding_curve: Box<Account<'info, BondingCurve>>,
    #[account(mut, associated_token::mint = mint, associated_token::authority = bonding_curve)]
    pub curve_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(seeds = [V4_CONFIG_SEED], bump, constraint = config.enabled @ BeamError::MigrationNotEnabled)]
    pub config: Box<Account<'info, BeamAmmV4Config>>,
    /// CHECK: fixed executable program; never frontend-selected.
    #[account(address = AMM_V4_PROGRAM, executable)]
    pub amm_program: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium config PDA; owner, length and fee read before CPI.
    pub amm_config: UncheckedAccount<'info>,
    /// CHECK: system-owned signer PDA that funds and owns the Raydium sources.
    #[account(mut, seeds = [V4_AUTH_SEED, mint.key().as_ref()], bump, owner = anchor_lang::system_program::ID)]
    pub migrator: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = user, associated_token::mint = mint, associated_token::authority = migrator)]
    pub coin_source: Box<Account<'info, TokenAccount>>,
    #[account(address = token::spl_token::native_mint::ID)]
    pub pc_mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = user, associated_token::mint = pc_mint, associated_token::authority = migrator)]
    pub pc_source: Box<Account<'info, TokenAccount>>,
    /// CHECK: canonical Raydium pool PDA; must be uncreated.
    #[account(mut)]
    pub pool: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium authority PDA.
    pub amm_authority: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium LP mint PDA; must be uncreated.
    #[account(mut)]
    pub lp_mint: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium coin vault PDA; must be uncreated.
    #[account(mut)]
    pub coin_vault: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium pc vault PDA; must be uncreated.
    #[account(mut)]
    pub pc_vault: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium target-orders PDA; must be uncreated.
    #[account(mut)]
    pub target_orders: UncheckedAccount<'info>,
    /// CHECK: Raydium's fixed WSOL create-pool fee account for this network.
    #[account(mut)]
    pub fee_account: UncheckedAccount<'info>,
    /// CHECK: canonical ATA of the migration authority for the LP mint; Raydium creates it.
    #[account(mut)]
    pub lp_account: UncheckedAccount<'info>,
    #[account(init, payer = user, space = 8 + 3 * 32 + 8 * 7 + 1, seeds = [V4_RECEIPT_SEED, mint.key().as_ref()], bump)]
    pub receipt: Box<Account<'info, BeamAmmV4Receipt>>,
    #[account(init, payer = user, space = V4_RIGHTS_SPACE, seeds = [V4_RIGHTS_SEED, pool.key().as_ref()], bump)]
    pub rights: Box<Account<'info, BeamAmmV4Rights>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

/// The curve's recorded state that fixes the migration amounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanInput {
    pub vault_tokens: u64,
    pub supply: u64,
    pub real_quote: u64,
    pub post_quote: u64,
    pub migration_fee: u64,
    pub curve_lamports: u64,
    pub curve_rent: u64,
    pub creator_fee: u64,
    pub protocol_fees: u64,
    pub decimals: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub coin_amount: u64,
    pub pc_amount: u64,
    pub migration_fee: u64,
    pub floor: u64,
    pub user_lp: u64,
    pub locked_lp: u64,
}

/// What one call asks for, plus the bumps Anchor already found.
struct Req {
    mint: Pubkey,
    open_time: u64,
    max_pool_fee: u64,
    budget: u64,
    curve_bump: u8,
    auth_bump: u8,
}

/// Facts fixed by `check`, before anything moves.
struct Checked {
    pool: abi::PoolAddresses,
    pool_fee: u64,
    setup: u64,
    plan: Plan,
}

/// The migration authority's balances before the call, so donations are kept.
struct Before {
    lamports: u64,
    coin: u64,
    pc: u64,
    fee_account: u64,
}

/// Fixes the migration amounts from the curve's recorded state and checks it can cover its liabilities.
pub fn plan(i: PlanInput) -> Result<Plan> {
    require!(i.vault_tokens > 0 && i.vault_tokens <= i.supply, BeamError::InvalidMigrationBalance);
    let (total_quote, pc_amount) = migration_quote(i.real_quote, i.post_quote, i.migration_fee)?;
    let floor = protected_curve_floor(i.curve_lamports, i.curve_rent, i.creator_fee, i.protocol_fees, total_quote)?;
    let locked = locked_lp(i.decimals)?;
    Ok(Plan {
        coin_amount: i.vault_tokens,
        pc_amount,
        migration_fee: i.migration_fee,
        floor,
        user_lp: lp_out(i.vault_tokens, pc_amount, locked)?,
        locked_lp: locked,
    })
}

/// Exact lamports `Initialize2` takes from the wallet: the create-pool fee plus rent for the six accounts it creates.
pub fn setup_cost(create_pool_fee: u64, rent: &Rent) -> Result<u64> {
    let token_account = rent.minimum_balance(token::spl_token::state::Account::LEN);
    [
        rent.minimum_balance(AMM_INFO_LEN),
        rent.minimum_balance(TARGET_ORDERS_LEN),
        rent.minimum_balance(token::spl_token::state::Mint::LEN),
        token_account,
        token_account,
        token_account,
    ]
    .into_iter()
    .try_fold(create_pool_fee, u64::checked_add)
    .ok_or_else(|| error!(PumpError::Overflow))
}

/// The `10^decimals` LP that Raydium locks for good in a new pool.
pub fn locked_lp(decimals: u8) -> Result<u64> {
    10u64.checked_pow(u32::from(decimals)).ok_or_else(|| error!(PumpError::Overflow))
}

/// LP that Raydium issues to the wallet: `isqrt(coin * pc)` minus the locked amount.
pub fn lp_out(coin: u64, pc: u64, locked_lp: u64) -> Result<u64> {
    let product = u128::from(coin).checked_mul(u128::from(pc)).ok_or(PumpError::Overflow)?;
    let liquidity = u64::try_from(product.isqrt()).map_err(|_| error!(PumpError::Overflow))?;
    let user_lp = liquidity.checked_sub(locked_lp).ok_or(BeamError::InvalidMigrationBalance)?;
    require!(user_lp > 0, BeamError::InvalidMigrationBalance);
    Ok(user_lp)
}

/// Creates the config, disabled, with the largest create-pool fee Beam will pay.
pub fn configure_v4(ctx: Context<ConfigureV4>, max_pool_fee: u64) -> Result<()> {
    let config = &mut ctx.accounts.config;
    config.enabled = false;
    config.raydium_program = AMM_V4_PROGRAM;
    config.max_pool_fee = max_pool_fee;
    Ok(())
}

/// Whether enabling is allowed: only a build with `experimental-amm-v4` can switch migration on.
#[must_use]
pub fn can_enable(enabled: bool) -> bool {
    !enabled || cfg!(feature = "experimental-amm-v4")
}

/// Turns migration on or off (authority only); switching it off always works.
pub fn enable_v4(ctx: Context<EnableV4>, enabled: bool) -> Result<()> {
    require!(can_enable(enabled), BeamError::MigrationNotEnabled);
    ctx.accounts.config.enabled = enabled;
    Ok(())
}

/// Fails with `err`, logging `why`, unless `ok`.
fn ensure(ok: bool, err: BeamError, why: &str) -> Result<()> {
    if ok {
        return Ok(());
    }
    msg!("amm v4: {}", why);
    Err(err.into())
}

/// Fails unless the account is exactly the expected address.
fn check_key(name: &str, account: &AccountInfo, expected: &Pubkey) -> Result<()> {
    if account.key != expected {
        msg!("amm v4: `{}` is not the derived address", name);
        return err!(BeamError::InvalidRaydiumAccount);
    }
    Ok(())
}

/// Fails unless the account has no lamports, no data and the system owner.
fn check_empty(name: &str, account: &AccountInfo) -> Result<()> {
    if account.lamports() != 0 || !account.data_is_empty() || account.owner != &anchor_lang::system_program::ID {
        msg!("amm v4: `{}` already exists", name);
        return err!(BeamError::InvalidRaydiumAccount);
    }
    Ok(())
}

/// Moves lamports directly between two accounts, with checked arithmetic.
fn move_lamports(from: &AccountInfo, to: &AccountInfo, amount: u64) -> Result<()> {
    require!(from.key != to.key, BeamError::InvalidRaydiumAccount);
    let left = from.lamports().checked_sub(amount).ok_or(BeamError::InvalidMigrationBalance)?;
    let right = to.lamports().checked_add(amount).ok_or(PumpError::Overflow)?;
    **from.try_borrow_mut_lamports()? = left;
    **to.try_borrow_mut_lamports()? = right;
    Ok(())
}

/// Reads an SPL token account owned by the token program.
fn unpack(account: &AccountInfo) -> Result<token::spl_token::state::Account> {
    require_keys_eq!(*account.owner, token::ID, BeamError::InvalidMigrationResult);
    Ok(token::spl_token::state::Account::unpack(&account.try_borrow_data()?)?)
}

/// Refuses mints that Raydium's initializer would overflow on.
fn check_decimals(decimals: u8) -> Result<()> {
    require!(decimals <= V4_MAX_DECIMALS, BeamError::UnsupportedMint);
    Ok(())
}

/// True when no two keys are equal.
fn distinct<'a>(keys: impl Iterator<Item = &'a Pubkey> + Clone) -> bool {
    let mut rest = keys;
    while let Some(key) = rest.next() {
        if rest.clone().any(|other| other == key) {
            return false;
        }
    }
    true
}

/// A source must be an initialized, unfrozen token account with no delegate or close authority; a balance is allowed.
fn check_source(state: &token::spl_token::state::Account, mint: &Pubkey, owner: &Pubkey, native: bool) -> Result<()> {
    require!(
        state.mint == *mint
            && state.owner == *owner
            && state.state == token::spl_token::state::AccountState::Initialized
            && state.delegate.is_none()
            && state.close_authority.is_none()
            && state.is_native.is_some() == native,
        BeamError::InvalidMigrationSource
    );
    Ok(())
}

/// Checks the request and every account and fixes the amounts, before anything moves.
fn check(a: &MigrateV4, req: &Req) -> Result<Checked> {
    require_keys_eq!(a.config.raydium_program, AMM_V4_PROGRAM, BeamError::InvalidRaydiumAccount);
    require!(a.global.enable_migrate, BeamError::MigrationNotEnabled);
    require!(req.max_pool_fee <= a.config.max_pool_fee, BeamError::CreatePoolFeeExceedsCap);
    let now = u64::try_from(Clock::get()?.unix_timestamp).map_err(|_| error!(BeamError::OpenTimeInFuture))?;
    require!(req.open_time <= now, BeamError::OpenTimeInFuture);
    require!(a.bonding_curve.complete && a.bonding_curve.real_token_reserves == 0, PumpError::BondingCurveNotComplete);
    crate::beam_policy::mayhem(a.bonding_curve.is_mayhem_mode)?;
    require_keys_eq!(a.bonding_curve.quote_mint, Pubkey::default(), PumpError::UnsupportedQuoteMint);

    let curve_info = a.bonding_curve.to_account_info();
    let authority = a.migrator.key();
    ensure(
        a.withdraw_authority.key != curve_info.key && a.withdraw_authority.key != &authority && a.user.key != &authority,
        BeamError::InvalidRaydiumAccount,
        "the fee recipient, the caller and the curve must differ from the migration authority",
    )?;

    let pool = abi::derive_pool_addresses(&AMM_V4_PROGRAM, &req.mint).map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    check_key("amm_config", &a.amm_config.to_account_info(), &pool.config)?;
    check_key("pool", &a.pool.to_account_info(), &pool.amm)?;
    check_key("amm_authority", &a.amm_authority.to_account_info(), &pool.authority)?;
    check_key("lp_mint", &a.lp_mint.to_account_info(), &pool.lp_mint)?;
    check_key("coin_vault", &a.coin_vault.to_account_info(), &pool.coin_vault)?;
    check_key("pc_vault", &a.pc_vault.to_account_info(), &pool.pc_vault)?;
    check_key("target_orders", &a.target_orders.to_account_info(), &pool.target_orders)?;
    check_key("fee_account", &a.fee_account.to_account_info(), &pool.create_pool_fee_destination)?;
    check_key("lp_account", &a.lp_account.to_account_info(), &abi::user_lp_ata(&authority, &pool.lp_mint))?;
    for (name, created) in [
        ("pool", &a.pool),
        ("lp_mint", &a.lp_mint),
        ("coin_vault", &a.coin_vault),
        ("pc_vault", &a.pc_vault),
        ("target_orders", &a.target_orders),
        ("lp_account", &a.lp_account),
    ] {
        check_empty(name, &created.to_account_info())?;
    }
    abi::validate_mints(&abi::Initialize2Accounts {
        coin_mint: abi::MintKey { key: req.mint, owner: *a.mint.to_account_info().owner },
        pc_mint: abi::MintKey { key: a.pc_mint.key(), owner: *a.pc_mint.to_account_info().owner },
        market_seed: req.mint,
        user_wallet: authority,
        user_coin_source: a.coin_source.key(),
        user_pc_source: a.pc_source.key(),
        user_lp_ata: a.lp_account.key(),
    })
    .map_err(|_| error!(BeamError::UnsupportedMint))?;
    check_decimals(a.mint.decimals)?;

    let pool_fee = abi::validate_create_pool_fee_account(&AMM_V4_PROGRAM, &a.amm_config.to_account_info(), req.max_pool_fee).map_err(|e| match e {
        abi::AmmV4AbiError::CreatePoolFeeExceedsCap => error!(BeamError::CreatePoolFeeExceedsCap),
        _ => error!(BeamError::InvalidRaydiumAccount),
    })?;
    let setup = setup_cost(pool_fee, &a.rent)?;
    let worst_case = setup_cost(req.max_pool_fee, &a.rent)?;
    require!(req.budget >= setup && req.budget <= worst_case, BeamError::SetupFundingOutOfBounds);

    let amounts = plan(PlanInput {
        vault_tokens: a.curve_vault.amount,
        supply: a.bonding_curve.token_total_supply,
        real_quote: a.bonding_curve.real_quote_reserves,
        post_quote: a.bonding_curve.post_complete_quote_in,
        migration_fee: a.global.pool_migration_fee,
        curve_lamports: curve_info.lamports(),
        curve_rent: a.rent.minimum_balance(curve_info.data_len()),
        creator_fee: a.bonding_curve.creator_fee,
        protocol_fees: a.bonding_curve.protocol_fees,
        decimals: a.mint.decimals,
    })?;
    Ok(Checked { pool, pool_fee, setup, plan: amounts })
}

/// Records the migration authority's balances so donations are kept, and checks its token accounts are clean.
fn snapshot(a: &MigrateV4, req: &Req, pool_fee: u64) -> Result<Before> {
    let authority = a.migrator.key();
    let pc_info = a.pc_source.to_account_info();
    let pc = unpack(&pc_info)?;
    check_source(&pc, &token::spl_token::native_mint::ID, &authority, true)?;
    let coin = unpack(&a.coin_source.to_account_info())?;
    check_source(&coin, &req.mint, &authority, false)?;
    let pc_reserve = pc.is_native.ok_or(BeamError::InvalidMigrationBalance)?;
    let fee_account = if pool_fee > 0 { unpack(&a.fee_account.to_account_info())?.amount } else { 0 };
    Ok(Before {
        lamports: a.migrator.lamports(),
        coin: coin.amount,
        pc: pc_info.lamports().checked_sub(pc_reserve).ok_or(BeamError::InvalidMigrationBalance)?,
        fee_account,
    })
}

/// Moves the setup money, tokens and SOL to the migration authority and syncs the wrapped SOL.
fn fund(a: &MigrateV4, c: &Checked, before: &Before, req: &Req) -> Result<()> {
    let curve_info = a.bonding_curve.to_account_info();
    let pc_info = a.pc_source.to_account_info();
    let curve_bump = [req.curve_bump];
    let curve_seeds: &[&[u8]] = &[b"bonding-curve", req.mint.as_ref(), &curve_bump];

    anchor_lang::system_program::transfer(
        CpiContext::new(
            a.system_program.to_account_info(),
            anchor_lang::system_program::Transfer { from: a.user.to_account_info(), to: a.migrator.to_account_info() },
        ),
        req.budget,
    )?;
    token::transfer_checked(
        CpiContext::new_with_signer(
            a.token_program.to_account_info(),
            TransferChecked { from: a.curve_vault.to_account_info(), mint: a.mint.to_account_info(), to: a.coin_source.to_account_info(), authority: curve_info.clone() },
            &[curve_seeds],
        ),
        c.plan.coin_amount,
        a.mint.decimals,
    )?;
    move_lamports(&curve_info, &pc_info, c.plan.pc_amount)?;
    let mut sync = token::spl_token::instruction::sync_native(&token::ID, &pc_info.key())?;
    sync.accounts.push(AccountMeta::new_readonly(a.rent.key(), false));
    sync.accounts.push(AccountMeta::new(curve_info.key(), false));
    invoke(&sync, &[pc_info.clone(), a.rent.to_account_info(), curve_info, a.token_program.to_account_info()])?;

    let pc_want = before.pc.checked_add(c.plan.pc_amount).ok_or(PumpError::Overflow)?;
    ensure(unpack(&pc_info)?.amount == pc_want, BeamError::InvalidMigrationBalance, "wrapped SOL balance is wrong after funding")?;
    let coin_want = before.coin.checked_add(c.plan.coin_amount).ok_or(PumpError::Overflow)?;
    ensure(unpack(&a.coin_source.to_account_info())?.amount == coin_want, BeamError::InvalidMigrationBalance, "token balance is wrong after funding")?;
    Ok(())
}

/// Calls Raydium's `Initialize2`, with the migration authority signing.
fn create_pool(a: &MigrateV4, c: &Checked, req: &Req) -> Result<()> {
    let bump = [req.auth_bump];
    let seeds: &[&[u8]] = &[V4_AUTH_SEED, req.mint.as_ref(), &bump];
    // (account, writable, signer) in Raydium's fixed 19-account order; slot 14 is the market seed, which is the mint.
    // Kept on the heap: a 19-entry array of AccountInfo would overflow the 4 KB SBF stack frame.
    let mut slots: Vec<(AccountInfo, bool, bool)> = Vec::with_capacity(abi::INITIALIZE2_ACCOUNT_COUNT);
    slots.push((a.token_program.to_account_info(), false, false));
    slots.push((a.associated_token_program.to_account_info(), false, false));
    slots.push((a.system_program.to_account_info(), false, false));
    slots.push((a.rent.to_account_info(), false, false));
    slots.push((a.pool.to_account_info(), true, false));
    slots.push((a.amm_authority.to_account_info(), false, false));
    slots.push((a.lp_mint.to_account_info(), true, false));
    slots.push((a.mint.to_account_info(), false, false));
    slots.push((a.pc_mint.to_account_info(), false, false));
    slots.push((a.coin_vault.to_account_info(), true, false));
    slots.push((a.pc_vault.to_account_info(), true, false));
    slots.push((a.target_orders.to_account_info(), true, false));
    slots.push((a.amm_config.to_account_info(), false, false));
    slots.push((a.fee_account.to_account_info(), true, false));
    slots.push((a.mint.to_account_info(), false, false));
    slots.push((a.migrator.to_account_info(), true, true));
    slots.push((a.coin_source.to_account_info(), true, false));
    slots.push((a.pc_source.to_account_info(), true, false));
    slots.push((a.lp_account.to_account_info(), true, false));
    let keys = slots.iter().enumerate().filter(|(slot, _)| *slot != MARKET_SEED_SLOT).map(|(_, (info, ..))| info.key);
    ensure(distinct(keys), BeamError::InvalidRaydiumAccount, "Initialize2 accounts must be distinct")?;

    let ix = Instruction {
        program_id: AMM_V4_PROGRAM,
        accounts: slots.iter().map(|(info, writable, signer)| AccountMeta { pubkey: *info.key, is_signer: *signer, is_writable: *writable }).collect(),
        data: abi::encode_initialize2(
            c.pool.nonce,
            abi::Initialize2Args { open_time: req.open_time, init_pc_amount: c.plan.pc_amount, init_coin_amount: c.plan.coin_amount },
        )
        .to_vec(),
    };
    let mut infos: Vec<AccountInfo> = Vec::with_capacity(slots.len().saturating_add(1));
    infos.extend(slots.iter().map(|(info, ..)| info.clone()));
    infos.push(a.amm_program.to_account_info());
    invoke_signed(&ix, &infos, &[seeds])?;
    Ok(())
}

/// Re-reads what Raydium created and requires it to match the plan exactly.
fn verify(a: &MigrateV4, c: &Checked, before: &Before, req: &Req) -> Result<()> {
    let bad = BeamError::InvalidMigrationResult;
    let pool_info = a.pool.to_account_info();
    require_keys_eq!(*pool_info.owner, AMM_V4_PROGRAM, BeamError::InvalidMigrationResult);
    let expected = AmmInfo {
        nonce: u64::from(c.pool.nonce),
        coin_decimals: u64::from(a.mint.decimals),
        pc_decimals: u64::from(a.pc_mint.decimals),
        pool_open_time: req.open_time,
        coin_vault: c.pool.coin_vault,
        pc_vault: c.pool.pc_vault,
        coin_mint: req.mint,
        pc_mint: a.pc_mint.key(),
        lp_mint: c.pool.lp_mint,
        market: req.mint,
        target_orders: c.pool.target_orders,
        lp_amount: c.plan.user_lp.checked_add(c.plan.locked_lp).ok_or(PumpError::Overflow)?,
        ..AmmInfo::FRESH
    };
    AmmInfo::parse(&pool_info.try_borrow_data()?)?.check(&expected)?;
    let orders = a.target_orders.to_account_info();
    ensure(*orders.owner == AMM_V4_PROGRAM && orders.data_len() == TARGET_ORDERS_LEN, bad, "target orders account has the wrong owner or size")?;
    for (name, vault, mint, amount) in [("coin_vault", &a.coin_vault, req.mint, c.plan.coin_amount), ("pc_vault", &a.pc_vault, a.pc_mint.key(), c.plan.pc_amount)] {
        let state = unpack(&vault.to_account_info())?;
        if state.owner != c.pool.authority || state.mint != mint || state.amount != amount {
            msg!("amm v4: `{}` does not hold the planned amount", name);
            return err!(BeamError::InvalidMigrationResult);
        }
    }
    ensure(unpack(&a.coin_source.to_account_info())?.amount == before.coin, bad, "the token source was not emptied exactly")?;
    ensure(unpack(&a.pc_source.to_account_info())?.amount == before.pc, bad, "the wrapped SOL source was not emptied exactly")?;
    require_keys_eq!(*a.lp_mint.owner, token::ID, BeamError::InvalidMigrationResult);
    let lp_mint = token::spl_token::state::Mint::unpack(&a.lp_mint.try_borrow_data()?)?;
    ensure(lp_mint.supply == c.plan.user_lp && lp_mint.mint_authority == Some(c.pool.authority).into(), bad, "LP mint supply or authority is wrong")?;
    let lp = unpack(&a.lp_account.to_account_info())?;
    ensure(lp.mint == c.pool.lp_mint && lp.owner == a.migrator.key() && lp.amount == c.plan.user_lp, bad, "LP account does not hold the planned LP")?;
    if c.pool_fee > 0 {
        let fee_want = before.fee_account.checked_add(c.pool_fee).ok_or(PumpError::Overflow)?;
        ensure(unpack(&a.fee_account.to_account_info())?.amount == fee_want, bad, "the create-pool fee was not paid exactly")?;
    }
    let charged = before.lamports.checked_add(req.budget).and_then(|funded| funded.checked_sub(a.migrator.lamports())).ok_or(bad)?;
    ensure(charged == c.setup, bad, "setup lamports spent differ from the exact setup cost")?;
    Ok(())
}

/// Burns exactly the LP Raydium issued to the migration authority; a failure rolls the whole migration back.
fn burn_lp(a: &MigrateV4, c: &Checked, req: &Req) -> Result<()> {
    let bump = [req.auth_bump];
    let seeds: &[&[u8]] = &[V4_AUTH_SEED, req.mint.as_ref(), &bump];
    token::burn(
        CpiContext::new_with_signer(
            a.token_program.to_account_info(),
            Burn { mint: a.lp_mint.to_account_info(), from: a.lp_account.to_account_info(), authority: a.migrator.to_account_info() },
            &[seeds],
        ),
        c.plan.user_lp,
    )?;
    let lp_mint = token::spl_token::state::Mint::unpack(&a.lp_mint.try_borrow_data()?)?;
    let lp = unpack(&a.lp_account.to_account_info())?;
    ensure(lp_mint.supply == 0 && lp.amount == 0, BeamError::InvalidMigrationResult, "LP was not burned in full")
}

/// Returns the unspent setup money to the caller.
fn refund(a: &MigrateV4, req: &Req, amount: u64) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let bump = [req.auth_bump];
    let seeds: &[&[u8]] = &[V4_AUTH_SEED, req.mint.as_ref(), &bump];
    anchor_lang::system_program::transfer(
        CpiContext::new_with_signer(
            a.system_program.to_account_info(),
            anchor_lang::system_program::Transfer { from: a.migrator.to_account_info(), to: a.user.to_account_info() },
            &[seeds],
        ),
        amount,
    )
}

/// Pays the inherited migration fee after the last CPI, then checks the curve keeps its floor.
fn pay_fee(a: &MigrateV4, c: &Checked) -> Result<()> {
    let curve_info = a.bonding_curve.to_account_info();
    move_lamports(&curve_info, &a.withdraw_authority.to_account_info(), c.plan.migration_fee)?;
    ensure(curve_info.lamports() >= c.plan.floor, BeamError::InvalidMigrationBalance, "the curve fell below its protected floor")
}

/// Writes the rights record and receipt, zeroes the curve's reserves and emits the event.
fn record(a: &mut MigrateV4, c: &Checked, req: &Req) {
    let creator = a.bonding_curve.creator;
    a.rights.set_inner(BeamAmmV4Rights {
        pool: c.pool.amm,
        mint: req.mint,
        market_seed: req.mint,
        creator,
        creator_payout: creator,
        donation_record: Pubkey::default(),
        revision: 0,
        paused: false,
    });
    a.receipt.set_inner(BeamAmmV4Receipt {
        mint: req.mint,
        pool: c.pool.amm,
        lp_mint: c.pool.lp_mint,
        coin_amount: c.plan.coin_amount,
        pc_amount: c.plan.pc_amount,
        migration_fee: c.plan.migration_fee,
        create_pool_fee: c.pool_fee,
        lp_amount: c.plan.user_lp,
        setup_spent: c.setup,
        open_time: req.open_time,
        lp_disposition: LP_BURNED,
    });
    a.bonding_curve.real_quote_reserves = 0;
    a.bonding_curve.real_token_reserves = 0;
    a.bonding_curve.virtual_quote_reserves = 0;
    a.bonding_curve.virtual_token_reserves = 0;
    emit!(BeamAmmV4Migration {
        mint: req.mint,
        pool: c.pool.amm,
        coin_amount: c.plan.coin_amount,
        pc_amount: c.plan.pc_amount,
        migration_fee: c.plan.migration_fee,
        create_pool_fee: c.pool_fee,
        lp_amount: c.plan.user_lp,
    });
}

/// Entry point: runs the steps above in order, and any failure rolls everything back.
pub fn migrate_v4(ctx: Context<MigrateV4>, open_time: u64, max_pool_fee: u64, budget: u64) -> Result<()> {
    let req = Req { mint: ctx.accounts.mint.key(), open_time, max_pool_fee, budget, curve_bump: ctx.bumps.bonding_curve, auth_bump: ctx.bumps.migrator };
    let c = check(ctx.accounts, &req)?;
    let before = snapshot(ctx.accounts, &req, c.pool_fee)?;

    fund(ctx.accounts, &c, &before, &req)?;
    create_pool(ctx.accounts, &c, &req)?;
    verify(ctx.accounts, &c, &before, &req)?;
    burn_lp(ctx.accounts, &c, &req)?;

    let unspent = budget.checked_sub(c.setup).ok_or(BeamError::SetupFundingOutOfBounds)?;
    refund(ctx.accounts, &req, unspent)?;
    pay_fee(ctx.accounts, &c)?;
    record(&mut *ctx.accounts, &c, &req);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PlanInput {
        PlanInput {
            vault_tokens: 793_100_000_000_000,
            supply: 1_000_000_000_000_000,
            real_quote: 85_000_000_000,
            post_quote: 500,
            migration_fee: 1_000_000_000,
            curve_lamports: 100_000_000_000,
            curve_rent: 1_000_000,
            creator_fee: 3_000,
            protocol_fees: 2_000,
            decimals: 6,
        }
    }

    #[test]
    fn only_the_local_test_feature_can_enable_migration() {
        assert!(can_enable(false));
        assert_eq!(can_enable(true), cfg!(feature = "experimental-amm-v4"));
    }

    #[test]
    fn setup_cost_adds_rent() -> Result<()> {
        let rent = Rent::default();
        let rent_only = setup_cost(0, &rent)?;
        assert_eq!(
            rent_only,
            rent.minimum_balance(752) + rent.minimum_balance(2208) + rent.minimum_balance(82) + 3 * rent.minimum_balance(165)
        );
        assert_eq!(setup_cost(25_000_000, &rent)?, rent_only + 25_000_000);
        assert!(setup_cost(u64::MAX, &rent).is_err());
        Ok(())
    }

    #[test]
    fn lp_is_raydiums_integer_sqrt_minus_the_locked_amount() -> Result<()> {
        assert_eq!(lp_out(1_000_000_000_000, 1_000_000_000_000, 1_000_000)?, 1_000_000_000_000 - 1_000_000);
        assert!(lp_out(1, 1, 1_000_000).is_err());
        assert!(lp_out(u64::MAX, u64::MAX, 1_000_000).is_ok());
        assert_eq!(locked_lp(6)?, 1_000_000);
        Ok(())
    }

    #[test]
    fn plan_keeps_fee_liabilities_and_charges_the_migration_fee_once() -> Result<()> {
        let p = plan(sample())?;
        assert_eq!(p.pc_amount, 85_000_000_500 - 1_000_000_000);
        assert_eq!(p.floor, 1_000_000 + 3_000 + 2_000);
        assert_eq!(p.locked_lp, 1_000_000);
        assert!(plan(PlanInput { vault_tokens: 1, supply: 1, real_quote: 85_000_000_000, post_quote: 0, migration_fee: 0, curve_lamports: 85_000_000_000, curve_rent: 1_000_000, creator_fee: 0, protocol_fees: 0, decimals: 0 }).is_err());
        assert!(plan(PlanInput { vault_tokens: 2, supply: 1, ..sample() }).is_err());
        assert!(plan(PlanInput { vault_tokens: 0, ..sample() }).is_err());
        assert!(plan(PlanInput { vault_tokens: 1_000_000_000, supply: 1_000_000_000, real_quote: 10, post_quote: 0, migration_fee: 11, curve_lamports: u64::MAX, curve_rent: 0, creator_fee: 0, protocol_fees: 0, decimals: 0 }).is_err());
        Ok(())
    }

    #[test]
    fn mint_decimals_above_the_initializers_safe_range_are_refused() {
        assert!(check_decimals(0).is_ok());
        assert!(check_decimals(6).is_ok());
        assert!(check_decimals(V4_MAX_DECIMALS).is_ok());
        assert!(check_decimals(V4_MAX_DECIMALS + 1).is_err());
        assert!(check_decimals(u8::MAX).is_err());
        assert!(10u64.checked_pow(19).is_some());
        assert!(10u64.checked_pow(19).and_then(|x| x.checked_mul(5)).is_none());
    }

    #[test]
    fn duplicate_keys_are_detected() {
        let (a, b, c) = (Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique());
        assert!(distinct(std::iter::empty::<&Pubkey>()));
        assert!(distinct([a].iter()));
        assert!(distinct([a, b, c].iter()));
        assert!(!distinct([a, b, a].iter()));
        assert!(!distinct([a, a].iter()));
    }

    #[test]
    fn sources_must_be_plain_unfrozen_token_accounts() -> Result<()> {
        use token::spl_token::state::{Account as TokenAccount, AccountState};
        let (mint, owner) = (Pubkey::new_unique(), Pubkey::new_unique());
        let good = TokenAccount { mint, owner, amount: 7, state: AccountState::Initialized, ..TokenAccount::default() };
        check_source(&good, &mint, &owner, false)?;
        check_source(&TokenAccount { amount: u64::MAX, ..good }, &mint, &owner, false)?;
        let native = TokenAccount { is_native: Some(2_039_280).into(), ..good };
        check_source(&native, &mint, &owner, true)?;
        for bad in [
            TokenAccount { state: AccountState::Frozen, ..good },
            TokenAccount { state: AccountState::Uninitialized, ..good },
            TokenAccount { delegate: Some(Pubkey::new_unique()).into(), ..good },
            TokenAccount { close_authority: Some(Pubkey::new_unique()).into(), ..good },
            TokenAccount { mint: Pubkey::new_unique(), ..good },
            TokenAccount { owner: Pubkey::new_unique(), ..good },
            native,
        ] {
            assert!(check_source(&bad, &mint, &owner, false).is_err());
        }
        assert!(check_source(&good, &mint, &owner, true).is_err(), "a non-native account is not a WSOL source");
        Ok(())
    }
}
