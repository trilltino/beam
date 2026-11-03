//! Gated Raydium AMM v4 migration. Production builds cannot enable it.
//!
//! ABI source: raydium-io/raydium-amm 91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6,
//! the 19-account `Initialize2` path (see `beam_amm_v4`). The superseded CPMM
//! adapter in `beam_raydium` is not used here.
//!
//! LP tokens are received by the migration-authority PDA and stay there. This
//! module never burns, transfers or locks them: LP custody and fee rights are an
//! unresolved product decision, and no Beam instruction can move them yet.
//!
//! # Trust model
//!
//! Anyone may call [`execute_migration`] and pays for it, so the caller is treated as
//! hostile. Every Raydium address is derived from the mint rather than accepted from the
//! caller, the pool must not exist yet, the caller's funding is bounded above and below
//! ([`setup_cost`]), the pool may not be scheduled to open later than the current time, and
//! the result is authenticated after the CPI instead of trusted. Any failure unwinds the
//! whole transaction.
#![allow(
    // Anchor handlers take `Context` by value by convention, even when not all of it is used.
    clippy::needless_pass_by_value
)]
use crate::{
    beam_amm_v4 as abi,
    beam_policy::BeamError,
    beam_raydium::{migration_quote, protected_curve_floor},
    trade_types::BondingCurve,
    Global, PumpError,
};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::AccountMeta,
    program::{invoke, invoke_signed},
    program_pack::Pack,
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, TransferChecked},
};

/// Seed of the singleton [`BeamAmmV4Config`] account.
pub const AMM_V4_CONFIG_SEED: &[u8] = b"beam-amm-v4-config";
/// Seed of the system-owned PDA that funds and owns the Raydium source accounts, per mint.
pub const AMM_V4_MIGRATION_AUTHORITY_SEED: &[u8] = b"amm-v4-migration-authority";
/// Seed of the per-mint [`BeamAmmV4Receipt`].
pub const AMM_V4_RECEIPT_SEED: &[u8] = b"beam-amm-v4-migration";
/// Seed of the per-pool [`BeamAmmV4Rights`] record that the AMM v4 quote reader in draft PR #49 requires.
pub const AMM_V4_RIGHTS_SEED: &[u8] = b"beam-amm-rights";
/// Account space of [`BeamAmmV4Rights`]: discriminator, six keys, revision, paused.
pub const AMM_V4_RIGHTS_SPACE: usize = 8 + 6 * 32 + 8 + 1;
/// `AmmStatus::SwapOnly` in the pinned source: the pool trades and nothing is scheduled.
const STATUS_SWAP_ONLY: u64 = 6;
/// Size of the pinned packed `AmmInfo` account.
const AMM_INFO_LEN: usize = 752;
/// Size of the pinned packed `TargetOrders` account.
const TARGET_ORDERS_LEN: usize = 2208;
/// [`BeamAmmV4Receipt::lp_disposition`]: LP is held by the migration PDA; no release path.
pub const LP_HELD_UNRESOLVED: u8 = 0;

/// The Raydium AMM v4 program this build migrates into (devnet id with `raydium-devnet`).
#[cfg(feature = "raydium-devnet")]
pub const AMM_V4_PROGRAM: Pubkey = abi::DEVNET_PROGRAM_ID;
/// The Raydium AMM v4 program this build migrates into (devnet id with `raydium-devnet`).
#[cfg(not(feature = "raydium-devnet"))]
pub const AMM_V4_PROGRAM: Pubkey = abi::MAINNET_PROGRAM_ID;

/// Singleton migration settings, created disabled by [`configure_migration`].
#[account]
pub struct BeamAmmV4Config {
    /// Whether `migrate_amm_v4` may run. Only a build with `experimental-amm-v4` can set it.
    pub enabled: bool,
    /// The Raydium program this config was created for.
    pub raydium_program: Pubkey,
    /// Largest Raydium create-pool fee Beam will pay from the setup budget.
    pub max_create_pool_fee: u64,
}

/// Byte-for-byte the layout of `AmmV4Rights` in draft PR #49 (`beam_amm_v4_companion.rs`, head
/// 32c661d), including its discriminator, `sha256("account:AmmV4Rights")[..8]`. Migration writes it
/// so a graduated token is quotable without a separate `register` call. When #49 lands, delete this
/// type and use its `AmmV4Rights`; the bytes are identical, but #49's `register` would then fail
/// with an account-already-in-use error for a migrated pool, which is the intended outcome.
#[account(discriminator = [88, 155, 79, 159, 221, 232, 159, 98])]
pub struct BeamAmmV4Rights {
    /// The Raydium pool this record belongs to.
    pub pool: Pubkey,
    /// The graduated token's mint.
    pub mint: Pubkey,
    /// The seed Raydium used for the pool address; the mint itself.
    pub market_seed: Pubkey,
    /// The curve's creator.
    pub creator: Pubkey,
    /// Where creator payouts go; starts as the creator.
    pub creator_payout: Pubkey,
    /// Selected donation record; unset at graduation.
    pub donation_record: Pubkey,
    /// Bumped on every change to the record.
    pub revision: u64,
    /// Whether payouts against this record are paused.
    pub paused: bool,
}

/// Proof of one completed migration, one per mint.
#[account]
pub struct BeamAmmV4Receipt {
    /// The graduated token's mint.
    pub mint: Pubkey,
    /// The Raydium pool that was created.
    pub pool: Pubkey,
    /// The pool's LP mint.
    pub lp_mint: Pubkey,
    /// Tokens moved into the pool.
    pub coin_amount: u64,
    /// Lamports of wrapped SOL moved into the pool.
    pub pc_amount: u64,
    /// Inherited migration fee paid to `Global.withdraw_authority`.
    pub migration_fee: u64,
    /// Raydium's create-pool fee paid from the setup budget.
    pub create_pool_fee: u64,
    /// LP tokens issued to the migration PDA.
    pub lp_amount: u64,
    /// Exact lamports of the caller's setup budget that Raydium consumed.
    pub setup_spent: u64,
    /// The `open_time` passed to Raydium; never later than the migration slot's clock.
    pub open_time: u64,
    /// What happened to the LP tokens; see [`LP_HELD_UNRESOLVED`].
    pub lp_disposition: u8,
}

/// Emitted once per successful migration.
#[event]
pub struct BeamAmmV4Migration {
    /// The graduated token's mint.
    pub mint: Pubkey,
    /// The Raydium pool that was created.
    pub pool: Pubkey,
    /// Tokens moved into the pool.
    pub coin_amount: u64,
    /// Lamports of wrapped SOL moved into the pool.
    pub pc_amount: u64,
    /// Inherited migration fee paid out.
    pub migration_fee: u64,
    /// Raydium's create-pool fee paid.
    pub create_pool_fee: u64,
    /// LP tokens issued to the migration PDA.
    pub lp_amount: u64,
}

/// Accounts for [`configure_migration`].
#[derive(Accounts)]
pub struct ConfigureAmmV4<'info> {
    /// Beam's global settings; its `authority` must sign.
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    /// The `Global` authority, paying for the config account.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The singleton config, created here.
    #[account(init, payer = authority, space = 8 + 1 + 32 + 8, seeds = [AMM_V4_CONFIG_SEED], bump)]
    pub config: Account<'info, BeamAmmV4Config>,
    /// The system program, used to create the config.
    pub system_program: Program<'info, System>,
}

/// Accounts for [`set_migration_enabled`].
#[derive(Accounts)]
pub struct SetAmmV4Enabled<'info> {
    /// Beam's global settings; its `authority` must sign.
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    /// The `Global` authority.
    pub authority: Signer<'info>,
    /// The singleton config to update.
    #[account(mut, seeds = [AMM_V4_CONFIG_SEED], bump)]
    pub config: Account<'info, BeamAmmV4Config>,
}

/// Accounts for [`execute_migration`]. Every Raydium account is checked against an address
/// derived from the mint before use; none is trusted because the caller supplied it.
#[derive(Accounts)]
pub struct MigrateAmmV4<'info> {
    /// Beam's global settings; names the migration-fee recipient.
    #[account(seeds = [b"global"], bump, has_one = withdraw_authority)]
    pub global: Box<Account<'info, Global>>,
    /// CHECK: constrained to the recipient configured in Global.
    #[account(mut)]
    pub withdraw_authority: UncheckedAccount<'info>,
    /// Classic SPL Token mints only; a Token-2022 mint fails account validation.
    pub mint: Box<Account<'info, Mint>>,
    /// The completed curve being graduated.
    #[account(mut, seeds = [b"bonding-curve", mint.key().as_ref()], bump)]
    pub bonding_curve: Box<Account<'info, BondingCurve>>,
    /// The curve's token vault, which supplies the pool's token side.
    #[account(mut, associated_token::mint = mint, associated_token::authority = bonding_curve)]
    pub curve_token_vault: Box<Account<'info, TokenAccount>>,
    /// The caller, who pays the setup budget and all new account rent.
    #[account(mut)]
    pub user: Signer<'info>,
    /// The singleton config; must be enabled.
    #[account(seeds = [AMM_V4_CONFIG_SEED], bump, constraint = config.enabled @ BeamError::MigrationNotEnabled)]
    pub config: Box<Account<'info, BeamAmmV4Config>>,
    /// CHECK: fixed executable program; never frontend-selected.
    #[account(address = AMM_V4_PROGRAM, executable)]
    pub amm_program: UncheckedAccount<'info>,
    /// CHECK: canonical Raydium config PDA; owner, length and fee read before CPI.
    pub amm_config: UncheckedAccount<'info>,
    /// CHECK: system-owned signer PDA that funds and owns the Raydium sources.
    #[account(mut, seeds = [AMM_V4_MIGRATION_AUTHORITY_SEED, mint.key().as_ref()], bump, owner = anchor_lang::system_program::ID)]
    pub migration_authority: UncheckedAccount<'info>,
    /// The migration PDA's token account that supplies the pool's token side.
    #[account(init_if_needed, payer = user, associated_token::mint = mint, associated_token::authority = migration_authority)]
    pub authority_base: Box<Account<'info, TokenAccount>>,
    /// Wrapped SOL, the pool's quote side.
    #[account(address = token::spl_token::native_mint::ID)]
    pub wsol_mint: Box<Account<'info, Mint>>,
    /// The migration PDA's wrapped-SOL account that supplies the pool's quote side.
    #[account(init_if_needed, payer = user, associated_token::mint = wsol_mint, associated_token::authority = migration_authority)]
    pub authority_wsol: Box<Account<'info, TokenAccount>>,
    /// CHECK: canonical Raydium pool PDA; must be uncreated.
    #[account(mut)]
    pub amm_pool: UncheckedAccount<'info>,
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
    pub create_pool_fee_destination: UncheckedAccount<'info>,
    /// CHECK: canonical ATA of the migration authority for the LP mint; Raydium creates it.
    #[account(mut)]
    pub authority_lp: UncheckedAccount<'info>,
    /// The migration receipt, created here; its existence makes a replay fail.
    #[account(init, payer = user, space = 8 + 3 * 32 + 8 * 7 + 1, seeds = [AMM_V4_RECEIPT_SEED, mint.key().as_ref()], bump)]
    pub receipt: Box<Account<'info, BeamAmmV4Receipt>>,
    /// Keyed by the pool address; the handler requires `amm_pool` to equal the derived pool.
    #[account(init, payer = user, space = AMM_V4_RIGHTS_SPACE, seeds = [AMM_V4_RIGHTS_SEED, amm_pool.key().as_ref()], bump)]
    pub rights: Box<Account<'info, BeamAmmV4Rights>>,
    /// The SPL Token program.
    pub token_program: Program<'info, Token>,
    /// The associated-token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The Rent sysvar.
    pub rent: Sysvar<'info, Rent>,
}

/// Amounts fixed from authenticated Beam state before any lamport or token moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    /// Tokens that go into the pool: everything in the curve vault.
    pub coin_amount: u64,
    /// Lamports of wrapped SOL that go into the pool, after the migration fee.
    pub pc_amount: u64,
    /// Inherited migration fee paid to `Global.withdraw_authority`.
    pub migration_fee: u64,
    /// Lamports the curve must still hold afterwards (rent, fee claims, unrelated balances).
    pub protected_floor: u64,
    /// LP tokens Raydium will issue to the migration PDA.
    pub user_lp: u64,
}

/// Fixes the migration amounts from the curve's recorded state.
///
/// # Errors
///
/// Fails when the vault is empty or exceeds the token supply, when the quote cannot cover the
/// migration fee or the curve's protected balances, or when the pool would issue no LP.
#[allow(clippy::too_many_arguments)]
pub fn migration_plan(
    vault_tokens: u64,
    token_total_supply: u64,
    real_quote: u64,
    post_complete_quote_in: u64,
    migration_fee: u64,
    curve_lamports: u64,
    curve_rent: u64,
    creator_fee: u64,
    protocol_fees: u64,
    coin_decimals: u8,
) -> Result<MigrationPlan> {
    require!(vault_tokens > 0 && vault_tokens <= token_total_supply, BeamError::InvalidMigrationBalance);
    let (total_quote, pc_amount) = migration_quote(real_quote, post_complete_quote_in, migration_fee)?;
    let protected_floor = protected_curve_floor(curve_lamports, curve_rent, creator_fee, protocol_fees, total_quote)?;
    Ok(MigrationPlan {
        coin_amount: vault_tokens,
        pc_amount,
        migration_fee,
        protected_floor,
        user_lp: expected_user_lp(vault_tokens, pc_amount, coin_decimals)?,
    })
}

/// Exactly what `Initialize2` takes from the wallet: the create-pool fee plus rent
/// for the six accounts it creates (pool, target orders, LP mint, both vaults, LP account).
///
/// # Errors
///
/// Fails on arithmetic overflow.
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

/// Raydium mints `isqrt(coin * pc) - 10^coin_decimals` LP to the user.
///
/// # Errors
///
/// Fails on overflow, or when the pool is too small to issue any LP after the locked minimum.
pub fn expected_user_lp(coin: u64, pc: u64, coin_decimals: u8) -> Result<u64> {
    let product = u128::from(coin).checked_mul(u128::from(pc)).ok_or(PumpError::Overflow)?;
    let liquidity = u64::try_from(product.isqrt()).map_err(|_| error!(PumpError::Overflow))?;
    let locked = 10u64.checked_pow(u32::from(coin_decimals)).ok_or(PumpError::Overflow)?;
    let user_lp = liquidity.checked_sub(locked).ok_or(BeamError::InvalidMigrationBalance)?;
    require!(user_lp > 0, BeamError::InvalidMigrationBalance);
    Ok(user_lp)
}

/// Creates the config disabled, with the largest create-pool fee Beam will pay.
///
/// # Errors
///
/// Fails if the config already exists or the signer is not the `Global` authority.
pub fn configure_migration(ctx: Context<ConfigureAmmV4>, max_create_pool_fee: u64) -> Result<()> {
    let config = &mut ctx.accounts.config;
    config.enabled = false;
    config.raydium_program = AMM_V4_PROGRAM;
    config.max_create_pool_fee = max_create_pool_fee;
    Ok(())
}

/// No production build can turn migration on. LP custody, fee rights and the
/// Token-2022 plan are undecided, so the feature exists for local tests only.
#[must_use]
pub fn activation_permitted(enabled: bool) -> bool {
    !enabled || cfg!(feature = "experimental-amm-v4")
}

/// Turns migration on or off. Turning it off always works; turning it on needs the
/// `experimental-amm-v4` build feature.
///
/// # Errors
///
/// Fails when enabling in a build without `experimental-amm-v4`, or for a non-authority signer.
pub fn set_migration_enabled(ctx: Context<SetAmmV4Enabled>, enabled: bool) -> Result<()> {
    require!(activation_permitted(enabled), BeamError::MigrationNotEnabled);
    ctx.accounts.config.enabled = enabled;
    Ok(())
}

/// Fails unless `account` is exactly `expected`.
fn expect_key(account: &AccountInfo, expected: &Pubkey) -> Result<()> {
    require_keys_eq!(account.key(), *expected, BeamError::InvalidRaydiumAccount);
    Ok(())
}

/// Fails unless `account` has no lamports, no data and the system owner.
fn require_uncreated(account: &AccountInfo) -> Result<()> {
    require!(
        account.lamports() == 0 && account.data_is_empty() && account.owner == &anchor_lang::system_program::ID,
        BeamError::InvalidRaydiumAccount
    );
    Ok(())
}

/// Moves lamports between two accounts without a CPI. `from` must be owned by this program.
fn checked_move(from: &AccountInfo, to: &AccountInfo, amount: u64) -> Result<()> {
    require!(from.key != to.key, BeamError::InvalidRaydiumAccount);
    let left = from.lamports().checked_sub(amount).ok_or(BeamError::InvalidMigrationBalance)?;
    let right = to.lamports().checked_add(amount).ok_or(PumpError::Overflow)?;
    **from.try_borrow_mut_lamports()? = left;
    **to.try_borrow_mut_lamports()? = right;
    Ok(())
}

/// Unpacks an SPL token account owned by the token program.
fn token_state(account: &AccountInfo) -> Result<token::spl_token::state::Account> {
    require_keys_eq!(*account.owner, token::ID, BeamError::InvalidMigrationResult);
    Ok(token::spl_token::state::Account::unpack(&account.try_borrow_data()?)?)
}

/// Reads a little-endian `u64` at `at`, or `None` when the slice is too short.
fn read_u64(data: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(at..)?.get(..8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Everything decided before any lamport or token moves.
struct Preflight {
    /// Raydium addresses derived from the mint.
    pool: abi::PoolAddresses,
    /// Raydium's create-pool fee, read from its canonical config just now.
    create_pool_fee: u64,
    /// Exact lamports `Initialize2` will take from the migration PDA.
    setup_cost: u64,
    /// Amounts and the curve's protected balance.
    plan: MigrationPlan,
}

/// Balances captured before funding, so donations are neither adopted nor swept.
struct Baseline {
    /// The migration PDA's lamports before the setup budget arrives.
    authority_lamports: u64,
    /// Tokens already in the migration PDA's base account.
    base: u64,
    /// Wrapped-SOL tokens already spendable in the migration PDA's WSOL account.
    wsol_spendable: u64,
    /// Tokens in Raydium's fee destination before the fee is paid.
    fee_destination: u64,
}

/// Validates the request and every account, and fixes the amounts. Nothing moves here.
fn preflight(a: &MigrateAmmV4, open_time: u64, max_create_pool_fee: u64, setup_budget: u64) -> Result<Preflight> {
    require_keys_eq!(a.config.raydium_program, AMM_V4_PROGRAM, BeamError::InvalidRaydiumAccount);
    require!(a.global.enable_migrate, BeamError::MigrationNotEnabled);
    require!(max_create_pool_fee <= a.config.max_create_pool_fee, BeamError::CreatePoolFeeExceedsCap);
    // A far-future open time would leave the pool unable to trade, and nobody could redo it.
    let now = u64::try_from(Clock::get()?.unix_timestamp).map_err(|_| error!(BeamError::OpenTimeInFuture))?;
    require!(open_time <= now, BeamError::OpenTimeInFuture);
    require!(a.bonding_curve.complete && a.bonding_curve.real_token_reserves == 0, PumpError::BondingCurveNotComplete);
    crate::beam_policy::mayhem(a.bonding_curve.is_mayhem_mode)?;
    require_keys_eq!(a.bonding_curve.quote_mint, Pubkey::default(), PumpError::UnsupportedQuoteMint);

    let curve_info = a.bonding_curve.to_account_info();
    let authority_key = a.migration_authority.key();
    require!(a.withdraw_authority.key != curve_info.key && a.withdraw_authority.key != &authority_key, BeamError::InvalidRaydiumAccount);
    require!(a.user.key != &authority_key, BeamError::InvalidRaydiumAccount);

    // Every Raydium identity is derived here. The mint is the AMM seed, so the
    // pool address is deterministic and bound to this launch.
    let mint = a.mint.key();
    let pool = abi::derive_pool_addresses(&AMM_V4_PROGRAM, &mint).map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    expect_key(&a.amm_config.to_account_info(), &pool.config)?;
    expect_key(&a.amm_pool.to_account_info(), &pool.amm)?;
    expect_key(&a.amm_authority.to_account_info(), &pool.authority)?;
    expect_key(&a.lp_mint.to_account_info(), &pool.lp_mint)?;
    expect_key(&a.coin_vault.to_account_info(), &pool.coin_vault)?;
    expect_key(&a.pc_vault.to_account_info(), &pool.pc_vault)?;
    expect_key(&a.target_orders.to_account_info(), &pool.target_orders)?;
    expect_key(&a.create_pool_fee_destination.to_account_info(), &pool.create_pool_fee_destination)?;
    expect_key(&a.authority_lp.to_account_info(), &abi::user_lp_ata(&authority_key, &pool.lp_mint))?;
    for source in [&a.amm_pool, &a.lp_mint, &a.coin_vault, &a.pc_vault, &a.target_orders, &a.authority_lp] {
        require_uncreated(&source.to_account_info())?;
    }
    abi::validate_mints(&initialize2_accounts(a, &mint, *a.mint.to_account_info().owner, *a.wsol_mint.to_account_info().owner))
        .map_err(|_| error!(BeamError::UnsupportedMint))?;

    // The fee is read from Raydium's own config immediately before the CPI. The budget must cover
    // the real cost and may not exceed the worst case the caller accepted: rent is fixed by the
    // pinned account sizes, so only the fee can vary.
    let create_pool_fee = abi::validate_create_pool_fee_account(&AMM_V4_PROGRAM, &a.amm_config.to_account_info(), max_create_pool_fee)
        .map_err(|e| match e {
            abi::AmmV4AbiError::CreatePoolFeeExceedsCap => error!(BeamError::CreatePoolFeeExceedsCap),
            _ => error!(BeamError::InvalidRaydiumAccount),
        })?;
    let required_setup = setup_cost(create_pool_fee, &a.rent)?;
    let worst_case_setup = setup_cost(max_create_pool_fee, &a.rent)?;
    require!(setup_budget >= required_setup && setup_budget <= worst_case_setup, BeamError::SetupFundingOutOfBounds);

    let plan = migration_plan(
        a.curve_token_vault.amount,
        a.bonding_curve.token_total_supply,
        a.bonding_curve.real_quote_reserves,
        a.bonding_curve.post_complete_quote_in,
        a.global.pool_migration_fee,
        curve_info.lamports(),
        a.rent.minimum_balance(curve_info.data_len()),
        a.bonding_curve.creator_fee,
        a.bonding_curve.protocol_fees,
        a.mint.decimals,
    )?;
    Ok(Preflight { pool, create_pool_fee, setup_cost: required_setup, plan })
}

/// The account set `Initialize2` is called with, for validation and for building the CPI.
fn initialize2_accounts(a: &MigrateAmmV4, mint: &Pubkey, mint_owner: Pubkey, wsol_owner: Pubkey) -> abi::Initialize2Accounts {
    abi::Initialize2Accounts {
        coin_mint: abi::MintKey { key: *mint, owner: mint_owner },
        pc_mint: abi::MintKey { key: a.wsol_mint.key(), owner: wsol_owner },
        market_seed: *mint,
        user_wallet: a.migration_authority.key(),
        user_coin_source: a.authority_base.key(),
        user_pc_source: a.authority_wsol.key(),
        user_lp_ata: a.authority_lp.key(),
    }
}

/// Records the balances that must be unchanged apart from the planned movement.
fn baseline(a: &MigrateAmmV4, create_pool_fee: u64) -> Result<Baseline> {
    let wsol_info = a.authority_wsol.to_account_info();
    let wsol_reserve = token_state(&wsol_info)?.is_native.ok_or(BeamError::InvalidMigrationBalance)?;
    let fee_destination = if create_pool_fee > 0 { token_state(&a.create_pool_fee_destination.to_account_info())?.amount } else { 0 };
    Ok(Baseline {
        authority_lamports: a.migration_authority.lamports(),
        base: a.authority_base.amount,
        wsol_spendable: wsol_info.lamports().checked_sub(wsol_reserve).ok_or(BeamError::InvalidMigrationBalance)?,
        fee_destination,
    })
}

/// Moves the setup budget, the pool's tokens and its wrapped SOL to the migration PDA.
fn fund_sources(a: &MigrateAmmV4, pre: &Preflight, before: &Baseline, mint: &Pubkey, curve_bump: u8, setup_budget: u64) -> Result<()> {
    let curve_info = a.bonding_curve.to_account_info();
    let wsol_info = a.authority_wsol.to_account_info();
    let curve_bump = [curve_bump];
    let curve_seeds: &[&[u8]] = &[b"bonding-curve", mint.as_ref(), &curve_bump];

    // Setup funding is the caller's bounded contribution, separate from liquidity.
    anchor_lang::system_program::transfer(
        CpiContext::new(
            a.system_program.to_account_info(),
            anchor_lang::system_program::Transfer { from: a.user.to_account_info(), to: a.migration_authority.to_account_info() },
        ),
        setup_budget,
    )?;
    token::transfer_checked(
        CpiContext::new_with_signer(
            a.token_program.to_account_info(),
            TransferChecked { from: a.curve_token_vault.to_account_info(), mint: a.mint.to_account_info(), to: a.authority_base.to_account_info(), authority: curve_info.clone() },
            &[curve_seeds],
        ),
        pre.plan.coin_amount,
        a.mint.decimals,
    )?;
    checked_move(&curve_info, &wsol_info, pre.plan.pc_amount)?;
    // Solana rejects a CPI while the caller's accounts are unbalanced, and only the CPI's own
    // accounts are synced. The curve is the other side of the lamport move above, so it rides
    // along. The newer token program reads the second SyncNative account as the Rent sysvar and
    // rejects anything else, so Rent goes second and the curve third; the older program ignores
    // extras.
    let mut sync = token::spl_token::instruction::sync_native(&token::ID, &wsol_info.key())?;
    sync.accounts.push(AccountMeta::new_readonly(a.rent.key(), false));
    sync.accounts.push(AccountMeta::new(curve_info.key(), false));
    invoke(&sync, &[wsol_info.clone(), a.rent.to_account_info(), curve_info, a.token_program.to_account_info()])?;

    let wsol_expected = before.wsol_spendable.checked_add(pre.plan.pc_amount).ok_or(PumpError::Overflow)?;
    require!(token_state(&wsol_info)?.amount == wsol_expected, BeamError::InvalidMigrationBalance);
    let base_expected = before.base.checked_add(pre.plan.coin_amount).ok_or(PumpError::Overflow)?;
    require!(token_state(&a.authority_base.to_account_info())?.amount == base_expected, BeamError::InvalidMigrationBalance);
    Ok(())
}

/// Calls Raydium `Initialize2` with the migration PDA as the signing wallet.
fn initialize_pool(a: &MigrateAmmV4, pre: &Preflight, mint: &Pubkey, open_time: u64, authority_bump: u8) -> Result<()> {
    let authority_bump = [authority_bump];
    let authority_seeds: &[&[u8]] = &[AMM_V4_MIGRATION_AUTHORITY_SEED, mint.as_ref(), &authority_bump];
    let ix = abi::initialize2_instruction(
        AMM_V4_PROGRAM,
        &initialize2_accounts(a, mint, token::ID, token::ID),
        abi::Initialize2Args { open_time, init_pc_amount: pre.plan.pc_amount, init_coin_amount: pre.plan.coin_amount },
    )
    .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let candidates = [
        a.token_program.to_account_info(),
        a.associated_token_program.to_account_info(),
        a.system_program.to_account_info(),
        a.rent.to_account_info(),
        a.amm_pool.to_account_info(),
        a.amm_authority.to_account_info(),
        a.lp_mint.to_account_info(),
        a.mint.to_account_info(),
        a.wsol_mint.to_account_info(),
        a.coin_vault.to_account_info(),
        a.pc_vault.to_account_info(),
        a.target_orders.to_account_info(),
        a.amm_config.to_account_info(),
        a.create_pool_fee_destination.to_account_info(),
        a.migration_authority.to_account_info(),
        a.authority_base.to_account_info(),
        a.authority_wsol.to_account_info(),
        a.authority_lp.to_account_info(),
        a.amm_program.to_account_info(),
    ];
    let mut infos = Vec::with_capacity(ix.accounts.len().saturating_add(1));
    for meta in &ix.accounts {
        infos.push(candidates.iter().find(|candidate| candidate.key == &meta.pubkey).ok_or(BeamError::InvalidRaydiumAccount)?.clone());
    }
    infos.push(a.amm_program.to_account_info());
    invoke_signed(&ix, &infos, &[authority_seeds])?;
    Ok(())
}

/// A successful CPI is not the result Beam wanted: authenticate the pool, vaults, LP and fee.
fn verify_pool(a: &MigrateAmmV4, pre: &Preflight, before: &Baseline, mint: &Pubkey, setup_budget: u64) -> Result<()> {
    let pool_info = a.amm_pool.to_account_info();
    require_keys_eq!(*pool_info.owner, AMM_V4_PROGRAM, BeamError::InvalidMigrationResult);
    {
        let pool = pool_info.try_borrow_data()?;
        require!(pool.len() == AMM_INFO_LEN, BeamError::InvalidMigrationResult);
        // Only SwapOnly: `open_time` is bounded by the clock, so the pool opens immediately.
        require!(read_u64(&pool, 0) == Some(STATUS_SWAP_ONLY), BeamError::InvalidMigrationResult);
    }
    for (vault, vault_mint, amount) in [(&a.coin_vault, *mint, pre.plan.coin_amount), (&a.pc_vault, a.wsol_mint.key(), pre.plan.pc_amount)] {
        let state = token_state(&vault.to_account_info())?;
        require!(state.owner == pre.pool.authority && state.mint == vault_mint && state.amount == amount, BeamError::InvalidMigrationResult);
    }
    require!(token_state(&a.authority_base.to_account_info())?.amount == before.base, BeamError::InvalidMigrationResult);
    require!(token_state(&a.authority_wsol.to_account_info())?.amount == before.wsol_spendable, BeamError::InvalidMigrationResult);
    require_keys_eq!(*a.lp_mint.owner, token::ID, BeamError::InvalidMigrationResult);
    let lp_mint = token::spl_token::state::Mint::unpack(&a.lp_mint.try_borrow_data()?)?;
    require!(lp_mint.supply == pre.plan.user_lp && lp_mint.mint_authority == Some(pre.pool.authority).into(), BeamError::InvalidMigrationResult);
    let lp = token_state(&a.authority_lp.to_account_info())?;
    require!(lp.mint == pre.pool.lp_mint && lp.owner == a.migration_authority.key() && lp.amount == pre.plan.user_lp, BeamError::InvalidMigrationResult);
    if pre.create_pool_fee > 0 {
        let fee_expected = before.fee_destination.checked_add(pre.create_pool_fee).ok_or(PumpError::Overflow)?;
        require!(token_state(&a.create_pool_fee_destination.to_account_info())?.amount == fee_expected, BeamError::InvalidMigrationResult);
    }
    // Raydium must have charged exactly the planned setup cost, no more and no less.
    let charged = before
        .authority_lamports
        .checked_add(setup_budget)
        .and_then(|funded| funded.checked_sub(a.migration_authority.lamports()))
        .ok_or(BeamError::InvalidMigrationResult)?;
    require!(charged == pre.setup_cost, BeamError::InvalidMigrationResult);
    Ok(())
}

/// Returns the unspent part of the setup budget to the caller.
fn refund_setup(a: &MigrateAmmV4, mint: &Pubkey, authority_bump: u8, refund: u64) -> Result<()> {
    if refund == 0 {
        return Ok(());
    }
    let authority_bump = [authority_bump];
    let authority_seeds: &[&[u8]] = &[AMM_V4_MIGRATION_AUTHORITY_SEED, mint.as_ref(), &authority_bump];
    anchor_lang::system_program::transfer(
        CpiContext::new_with_signer(
            a.system_program.to_account_info(),
            anchor_lang::system_program::Transfer { from: a.migration_authority.to_account_info(), to: a.user.to_account_info() },
            &[authority_seeds],
        ),
        refund,
    )
}

/// Pays the inherited migration fee. This is a direct lamport move, so it must come after the
/// last CPI (the runtime rejects a CPI while the caller's accounts are unbalanced). The curve
/// must still cover its rent and fee claims afterwards.
fn pay_migration_fee(a: &MigrateAmmV4, pre: &Preflight) -> Result<()> {
    let curve_info = a.bonding_curve.to_account_info();
    checked_move(&curve_info, &a.withdraw_authority.to_account_info(), pre.plan.migration_fee)?;
    require!(curve_info.lamports() >= pre.plan.protected_floor, BeamError::InvalidMigrationBalance);
    Ok(())
}

/// Writes the rights record and receipt, closes out the curve's reserves, and emits the event.
fn record(a: &mut MigrateAmmV4, pre: &Preflight, mint: Pubkey, open_time: u64) {
    // The quote reader needs a rights record per pool. It starts as #49's `register` would create
    // it: the curve's creator is both creator and payout wallet, with no donation selection.
    let creator = a.bonding_curve.creator;
    a.rights.set_inner(BeamAmmV4Rights {
        pool: pre.pool.amm,
        mint,
        market_seed: mint,
        creator,
        creator_payout: creator,
        donation_record: Pubkey::default(),
        revision: 0,
        paused: false,
    });
    a.receipt.set_inner(BeamAmmV4Receipt {
        mint,
        pool: pre.pool.amm,
        lp_mint: pre.pool.lp_mint,
        coin_amount: pre.plan.coin_amount,
        pc_amount: pre.plan.pc_amount,
        migration_fee: pre.plan.migration_fee,
        create_pool_fee: pre.create_pool_fee,
        lp_amount: pre.plan.user_lp,
        setup_spent: pre.setup_cost,
        open_time,
        lp_disposition: LP_HELD_UNRESOLVED,
    });
    a.bonding_curve.real_quote_reserves = 0;
    a.bonding_curve.real_token_reserves = 0;
    a.bonding_curve.virtual_quote_reserves = 0;
    a.bonding_curve.virtual_token_reserves = 0;
    emit!(BeamAmmV4Migration {
        mint,
        pool: pre.pool.amm,
        coin_amount: pre.plan.coin_amount,
        pc_amount: pre.plan.pc_amount,
        migration_fee: pre.plan.migration_fee,
        create_pool_fee: pre.create_pool_fee,
        lp_amount: pre.plan.user_lp,
    });
}

/// Graduates one completed curve into a new Raydium AMM v4 pool.
///
/// `open_time` may not be later than the current time. `max_create_pool_fee` is the most the caller
/// accepts for Raydium's fee, and `setup_budget` funds that fee plus the rent of the accounts
/// Raydium creates; any unspent part is refunded.
///
/// # Errors
///
/// Fails, with every effect rolled back, when migration is disabled, the curve is not a completed
/// native-SOL curve, any account differs from its derived address, the pool already exists, the fee
/// exceeds either cap, the budget is outside its bounds, `open_time` is in the future, Raydium
/// fails, or the pool Raydium created does not match the plan.
pub fn execute_migration(ctx: Context<MigrateAmmV4>, open_time: u64, max_create_pool_fee: u64, setup_budget: u64) -> Result<()> {
    let pre = preflight(ctx.accounts, open_time, max_create_pool_fee, setup_budget)?;
    let mint = ctx.accounts.mint.key();
    let before = baseline(ctx.accounts, pre.create_pool_fee)?;

    fund_sources(ctx.accounts, &pre, &before, &mint, ctx.bumps.bonding_curve, setup_budget)?;
    initialize_pool(ctx.accounts, &pre, &mint, open_time, ctx.bumps.migration_authority)?;
    verify_pool(ctx.accounts, &pre, &before, &mint, setup_budget)?;

    // `verify_pool` proved Raydium charged exactly `setup_cost`, so the rest of the budget is unspent.
    let refund = setup_budget.checked_sub(pre.setup_cost).ok_or(BeamError::SetupFundingOutOfBounds)?;
    refund_setup(ctx.accounts, &mint, ctx.bumps.migration_authority, refund)?;
    pay_migration_fee(ctx.accounts, &pre)?;
    record(&mut *ctx.accounts, &pre, mint, open_time);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_local_test_feature_can_enable_migration() {
        assert!(activation_permitted(false));
        assert_eq!(activation_permitted(true), cfg!(feature = "experimental-amm-v4"));
    }

    #[test]
    fn setup_cost_is_the_fee_plus_rent_for_the_six_created_accounts() -> Result<()> {
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
    fn lp_matches_raydium_integer_sqrt_minus_locked_minimum() -> Result<()> {
        // sqrt(10^12 * 10^12) = 10^12; 10^6 is locked for six decimals.
        assert_eq!(expected_user_lp(1_000_000_000_000, 1_000_000_000_000, 6)?, 1_000_000_000_000 - 1_000_000);
        assert!(expected_user_lp(1, 1, 6).is_err());
        assert!(expected_user_lp(u64::MAX, u64::MAX, 6).is_ok());
        Ok(())
    }

    #[test]
    fn plan_preserves_fee_liabilities_and_charges_the_migration_fee_once() -> Result<()> {
        // 85 SOL of quote, 1 SOL migration fee, 3 + 2 SOL of fee claims, 1 SOL rent.
        let p = migration_plan(793_100_000_000_000, 1_000_000_000_000_000, 85_000_000_000, 500, 1_000_000_000, 100_000_000_000, 1_000_000, 3_000, 2_000, 6)?;
        assert_eq!(p.pc_amount, 85_000_000_500 - 1_000_000_000);
        assert_eq!(p.protected_floor, 1_000_000 + 3_000 + 2_000);
        // Curve cannot cover recorded quote plus liabilities.
        assert!(migration_plan(1, 1, 85_000_000_000, 0, 0, 85_000_000_000, 1_000_000, 0, 0, 0).is_err());
        // More vault tokens than total supply, or an empty vault, never plan.
        assert!(migration_plan(2, 1, 1, 0, 0, u64::MAX, 0, 0, 0, 0).is_err());
        assert!(migration_plan(0, 1, 1, 0, 0, u64::MAX, 0, 0, 0, 0).is_err());
        // Migration fee larger than the quote leaves no pool quote.
        assert!(migration_plan(1_000_000_000, 1_000_000_000, 10, 0, 11, u64::MAX, 0, 0, 0, 0).is_err());
        Ok(())
    }

    #[test]
    fn read_u64_is_bounds_checked() {
        let data = [1u8, 0, 0, 0, 0, 0, 0, 0, 9];
        assert_eq!(read_u64(&data, 0), Some(1));
        assert_eq!(read_u64(&data, 1), Some(9 << 56), "eight bytes remain after offset 1");
        assert_eq!(read_u64(&data, 2), None, "only seven bytes remain after offset 2");
        assert_eq!(read_u64(&data, 9), None);
        assert_eq!(read_u64(&data, usize::MAX), None);
        assert_eq!(read_u64(&[], 0), None);
    }
}
