//! Gated Raydium AMM v4 migration. Production builds cannot enable it.
//!
//! ABI source: raydium-io/raydium-amm 91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6,
//! the 19-account `Initialize2` path (see `beam_amm_v4`). The superseded CPMM
//! adapter in `beam_raydium` is not used here.
//!
//! LP tokens are received by the migration-authority PDA and stay there. This
//! module never burns, transfers or locks them: LP custody and fee rights are an
//! unresolved product decision, and no Beam instruction can move them yet.
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

pub const AMM_V4_CONFIG_SEED: &[u8] = b"beam-amm-v4-config";
pub const AMM_V4_MIGRATION_AUTHORITY_SEED: &[u8] = b"amm-v4-migration-authority";
pub const AMM_V4_RECEIPT_SEED: &[u8] = b"beam-amm-v4-migration";
/// `AmmStatus::SwapOnly` and `AmmStatus::WaitingTrade` in the pinned source.
const STATUS_SWAP_ONLY: u64 = 6;
const STATUS_WAITING_TRADE: u64 = 7;
/// Size of the pinned packed `AmmInfo` account.
const AMM_INFO_LEN: usize = 752;
/// Size of the pinned packed `TargetOrders` account.
const TARGET_ORDERS_LEN: usize = 2208;
/// `receipt.lp_disposition`: LP is held by the migration PDA; no release path.
pub const LP_HELD_UNRESOLVED: u8 = 0;

#[cfg(feature = "raydium-devnet")]
pub const AMM_V4_PROGRAM: Pubkey = abi::DEVNET_PROGRAM_ID;
#[cfg(not(feature = "raydium-devnet"))]
pub const AMM_V4_PROGRAM: Pubkey = abi::MAINNET_PROGRAM_ID;

#[account]
pub struct BeamAmmV4Config {
    pub enabled: bool,
    pub raydium_program: Pubkey,
    /// Largest Raydium create-pool fee Beam will pay from the setup budget.
    pub max_create_pool_fee: u64,
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
pub struct ConfigureAmmV4<'info> {
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(init, payer = authority, space = 8 + 1 + 32 + 8, seeds = [AMM_V4_CONFIG_SEED], bump)]
    pub config: Account<'info, BeamAmmV4Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetAmmV4Enabled<'info> {
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    pub authority: Signer<'info>,
    #[account(mut, seeds = [AMM_V4_CONFIG_SEED], bump)]
    pub config: Account<'info, BeamAmmV4Config>,
}

#[derive(Accounts)]
pub struct MigrateAmmV4<'info> {
    #[account(seeds = [b"global"], bump, has_one = withdraw_authority)]
    pub global: Box<Account<'info, Global>>,
    /// CHECK: constrained to the recipient configured in Global.
    #[account(mut)]
    pub withdraw_authority: UncheckedAccount<'info>,
    /// Classic SPL Token mints only; a Token-2022 mint fails account validation.
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, seeds = [b"bonding-curve", mint.key().as_ref()], bump)]
    pub bonding_curve: Box<Account<'info, BondingCurve>>,
    #[account(mut, associated_token::mint = mint, associated_token::authority = bonding_curve)]
    pub curve_token_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub user: Signer<'info>,
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
    #[account(init_if_needed, payer = user, associated_token::mint = mint, associated_token::authority = migration_authority)]
    pub authority_base: Box<Account<'info, TokenAccount>>,
    #[account(address = token::spl_token::native_mint::ID)]
    pub wsol_mint: Box<Account<'info, Mint>>,
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
    #[account(init, payer = user, space = 8 + 3 * 32 + 8 * 7 + 1, seeds = [AMM_V4_RECEIPT_SEED, mint.key().as_ref()], bump)]
    pub receipt: Box<Account<'info, BeamAmmV4Receipt>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

/// Amounts fixed from authenticated Beam state before any lamport or token moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    pub coin_amount: u64,
    pub pc_amount: u64,
    pub migration_fee: u64,
    pub protected_floor: u64,
    pub user_lp: u64,
}

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
    .try_fold(create_pool_fee, |sum, part| sum.checked_add(part))
    .ok_or_else(|| error!(PumpError::Overflow))
}

/// Raydium mints `isqrt(coin * pc) - 10^coin_decimals` LP to the user.
pub fn expected_user_lp(coin: u64, pc: u64, coin_decimals: u8) -> Result<u64> {
    let product = u128::from(coin).checked_mul(u128::from(pc)).ok_or(PumpError::Overflow)?;
    let liquidity = u64::try_from(product.isqrt()).map_err(|_| error!(PumpError::Overflow))?;
    let locked = 10u64.checked_pow(u32::from(coin_decimals)).ok_or(PumpError::Overflow)?;
    let user_lp = liquidity.checked_sub(locked).ok_or(BeamError::InvalidMigrationBalance)?;
    require!(user_lp > 0, BeamError::InvalidMigrationBalance);
    Ok(user_lp)
}

pub fn configure_migration(ctx: Context<ConfigureAmmV4>, max_create_pool_fee: u64) -> Result<()> {
    let c = &mut ctx.accounts.config;
    c.enabled = false;
    c.raydium_program = AMM_V4_PROGRAM;
    c.max_create_pool_fee = max_create_pool_fee;
    Ok(())
}

/// No production build can turn migration on. LP custody, fee rights and the
/// Token-2022 plan are undecided, so the feature exists for local tests only.
pub fn activation_permitted(enabled: bool) -> bool {
    !enabled || cfg!(feature = "experimental-amm-v4")
}

pub fn set_migration_enabled(ctx: Context<SetAmmV4Enabled>, enabled: bool) -> Result<()> {
    require!(activation_permitted(enabled), BeamError::MigrationNotEnabled);
    ctx.accounts.config.enabled = enabled;
    Ok(())
}

fn expect_key(account: &AccountInfo, expected: &Pubkey) -> Result<()> {
    require_keys_eq!(account.key(), *expected, BeamError::InvalidRaydiumAccount);
    Ok(())
}

fn require_uncreated(account: &AccountInfo) -> Result<()> {
    require!(
        account.lamports() == 0 && account.data_is_empty() && account.owner == &anchor_lang::system_program::ID,
        BeamError::InvalidRaydiumAccount
    );
    Ok(())
}

fn checked_move(from: &AccountInfo, to: &AccountInfo, amount: u64) -> Result<()> {
    require!(from.key != to.key, BeamError::InvalidRaydiumAccount);
    let left = from.lamports().checked_sub(amount).ok_or(BeamError::InvalidMigrationBalance)?;
    let right = to.lamports().checked_add(amount).ok_or(PumpError::Overflow)?;
    **from.try_borrow_mut_lamports()? = left;
    **to.try_borrow_mut_lamports()? = right;
    Ok(())
}

fn token_state(account: &AccountInfo) -> Result<token::spl_token::state::Account> {
    require_keys_eq!(*account.owner, token::ID, BeamError::InvalidMigrationResult);
    Ok(token::spl_token::state::Account::unpack(&account.try_borrow_data()?)?)
}

pub fn execute_migration(ctx: Context<MigrateAmmV4>, open_time: u64, max_create_pool_fee: u64, setup_budget: u64) -> Result<()> {
    let a = &ctx.accounts;
    require_keys_eq!(a.config.raydium_program, AMM_V4_PROGRAM, BeamError::InvalidRaydiumAccount);
    require!(a.global.enable_migrate, BeamError::MigrationNotEnabled);
    require!(max_create_pool_fee <= a.config.max_create_pool_fee, BeamError::CreatePoolFeeExceedsCap);
    require!(a.bonding_curve.complete && a.bonding_curve.real_token_reserves == 0, PumpError::BondingCurveNotComplete);
    crate::beam_policy::mayhem(a.bonding_curve.is_mayhem_mode)?;
    require_keys_eq!(a.bonding_curve.quote_mint, Pubkey::default(), PumpError::UnsupportedQuoteMint);
    let curve_info = a.bonding_curve.to_account_info();
    let authority_info = a.migration_authority.to_account_info();
    require!(a.withdraw_authority.key != curve_info.key && a.withdraw_authority.key != authority_info.key, BeamError::InvalidRaydiumAccount);
    require!(a.user.key != authority_info.key, BeamError::InvalidRaydiumAccount);

    // Every Raydium identity is derived here. The mint is the AMM seed, so the
    // pool address is deterministic and bound to this launch.
    let mint = a.mint.key();
    let p = abi::derive_pool_addresses(&AMM_V4_PROGRAM, &mint).map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    expect_key(&a.amm_config.to_account_info(), &p.config)?;
    expect_key(&a.amm_pool.to_account_info(), &p.amm)?;
    expect_key(&a.amm_authority.to_account_info(), &p.authority)?;
    expect_key(&a.lp_mint.to_account_info(), &p.lp_mint)?;
    expect_key(&a.coin_vault.to_account_info(), &p.coin_vault)?;
    expect_key(&a.pc_vault.to_account_info(), &p.pc_vault)?;
    expect_key(&a.target_orders.to_account_info(), &p.target_orders)?;
    expect_key(&a.create_pool_fee_destination.to_account_info(), &p.create_pool_fee_destination)?;
    expect_key(&a.authority_lp.to_account_info(), &abi::user_lp_ata(&authority_info.key(), &p.lp_mint))?;
    for pda in [&a.amm_pool, &a.lp_mint, &a.coin_vault, &a.pc_vault, &a.target_orders, &a.authority_lp] {
        require_uncreated(&pda.to_account_info())?;
    }
    abi::validate_mints(&abi::Initialize2Accounts {
        coin_mint: abi::MintKey { key: mint, owner: *a.mint.to_account_info().owner },
        pc_mint: abi::MintKey { key: a.wsol_mint.key(), owner: *a.wsol_mint.to_account_info().owner },
        market_seed: mint,
        user_wallet: authority_info.key(),
        user_coin_source: a.authority_base.key(),
        user_pc_source: a.authority_wsol.key(),
        user_lp_ata: a.authority_lp.key(),
    })
    .map_err(|_| error!(BeamError::UnsupportedMint))?;

    // The fee is read from Raydium's own config immediately before the CPI.
    let create_pool_fee = abi::validate_create_pool_fee_account(&AMM_V4_PROGRAM, &a.amm_config.to_account_info(), max_create_pool_fee)
        .map_err(|e| match e {
            abi::AmmV4AbiError::CreatePoolFeeExceedsCap => error!(BeamError::CreatePoolFeeExceedsCap),
            _ => error!(BeamError::InvalidRaydiumAccount),
        })?;
    // The budget must cover the real cost and may not exceed the worst case the caller
    // accepted: rent is fixed by the pinned account sizes, so only the fee can vary.
    let rent = Rent::get()?;
    let required_setup = setup_cost(create_pool_fee, &rent)?;
    require!(
        setup_budget >= required_setup && setup_budget <= setup_cost(max_create_pool_fee, &rent)?,
        BeamError::SetupFundingOutOfBounds
    );

    let plan = migration_plan(
        a.curve_token_vault.amount,
        a.bonding_curve.token_total_supply,
        a.bonding_curve.real_quote_reserves,
        a.bonding_curve.post_complete_quote_in,
        a.global.pool_migration_fee,
        curve_info.lamports(),
        Rent::get()?.minimum_balance(curve_info.data_len()),
        a.bonding_curve.creator_fee,
        a.bonding_curve.protocol_fees,
        a.mint.decimals,
    )?;

    // Capture what is already there so donations are neither adopted nor swept.
    let authority_lamports_before = authority_info.lamports();
    let base_before = a.authority_base.amount;
    let wsol_info = a.authority_wsol.to_account_info();
    let wsol_reserve = token_state(&wsol_info)?.is_native.ok_or(BeamError::InvalidMigrationBalance)?;
    let wsol_spendable_before = wsol_info.lamports().checked_sub(wsol_reserve).ok_or(BeamError::InvalidMigrationBalance)?;
    let fee_dest_before = if create_pool_fee > 0 { token_state(&a.create_pool_fee_destination.to_account_info())?.amount } else { 0 };

    let curve_bump = [ctx.bumps.bonding_curve];
    let curve_seeds: &[&[u8]] = &[b"bonding-curve", mint.as_ref(), &curve_bump];
    let authority_bump = [ctx.bumps.migration_authority];
    let authority_seeds: &[&[u8]] = &[AMM_V4_MIGRATION_AUTHORITY_SEED, mint.as_ref(), &authority_bump];

    // Setup funding is the caller's bounded contribution, separate from liquidity.
    anchor_lang::system_program::transfer(
        CpiContext::new(a.system_program.to_account_info(), anchor_lang::system_program::Transfer { from: a.user.to_account_info(), to: authority_info.clone() }),
        setup_budget,
    )?;
    token::transfer_checked(
        CpiContext::new_with_signer(
            a.token_program.to_account_info(),
            TransferChecked { from: a.curve_token_vault.to_account_info(), mint: a.mint.to_account_info(), to: a.authority_base.to_account_info(), authority: curve_info.clone() },
            &[curve_seeds],
        ),
        plan.coin_amount,
        a.mint.decimals,
    )?;
    checked_move(&curve_info, &wsol_info, plan.pc_amount)?;
    // Solana rejects a CPI while the caller's accounts are unbalanced, and only the
    // CPI's own accounts are synced. The curve is the other side of the lamport move
    // above, so it rides along; SyncNative reads one account and ignores the rest.
    let mut sync = token::spl_token::instruction::sync_native(&token::ID, &wsol_info.key())?;
    sync.accounts.push(AccountMeta::new(curve_info.key(), false));
    invoke(&sync, &[wsol_info.clone(), curve_info.clone(), a.token_program.to_account_info()])?;
    let wsol_after_sync = token_state(&wsol_info)?.amount;
    require!(wsol_after_sync == wsol_spendable_before.checked_add(plan.pc_amount).ok_or(PumpError::Overflow)?, BeamError::InvalidMigrationBalance);
    require!(token_state(&a.authority_base.to_account_info())?.amount == base_before.checked_add(plan.coin_amount).ok_or(PumpError::Overflow)?, BeamError::InvalidMigrationBalance);

    let ix = abi::initialize2_instruction(
        AMM_V4_PROGRAM,
        &abi::Initialize2Accounts {
            coin_mint: abi::MintKey { key: mint, owner: token::ID },
            pc_mint: abi::MintKey { key: a.wsol_mint.key(), owner: token::ID },
            market_seed: mint,
            user_wallet: authority_info.key(),
            user_coin_source: a.authority_base.key(),
            user_pc_source: a.authority_wsol.key(),
            user_lp_ata: a.authority_lp.key(),
        },
        abi::Initialize2Args { open_time, init_pc_amount: plan.pc_amount, init_coin_amount: plan.coin_amount },
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
        authority_info.clone(),
        a.authority_base.to_account_info(),
        wsol_info.clone(),
        a.authority_lp.to_account_info(),
        a.amm_program.to_account_info(),
    ];
    let mut infos = Vec::with_capacity(candidates.len());
    for meta in ix.accounts.iter() {
        infos.push(candidates.iter().find(|c| c.key == &meta.pubkey).ok_or(BeamError::InvalidRaydiumAccount)?.clone());
    }
    infos.push(a.amm_program.to_account_info());
    invoke_signed(&ix, &infos, &[authority_seeds])?;

    // A successful CPI is not the result Beam wanted; authenticate the outcome.
    let pool_info = a.amm_pool.to_account_info();
    require_keys_eq!(*pool_info.owner, AMM_V4_PROGRAM, BeamError::InvalidMigrationResult);
    {
        let pool = pool_info.try_borrow_data()?;
        require!(pool.len() == AMM_INFO_LEN, BeamError::InvalidMigrationResult);
        let status = u64::from_le_bytes(pool[..8].try_into().map_err(|_| error!(BeamError::InvalidMigrationResult))?);
        require!(status == STATUS_SWAP_ONLY || status == STATUS_WAITING_TRADE, BeamError::InvalidMigrationResult);
    }
    for (vault, vault_mint, amount) in [(&a.coin_vault, mint, plan.coin_amount), (&a.pc_vault, a.wsol_mint.key(), plan.pc_amount)] {
        let state = token_state(&vault.to_account_info())?;
        require!(state.owner == p.authority && state.mint == vault_mint && state.amount == amount, BeamError::InvalidMigrationResult);
    }
    let base_after = token_state(&a.authority_base.to_account_info())?.amount;
    require!(base_after == base_before, BeamError::InvalidMigrationResult);
    require!(token_state(&wsol_info)?.amount == wsol_spendable_before, BeamError::InvalidMigrationResult);
    let lp_mint = token::spl_token::state::Mint::unpack(&a.lp_mint.try_borrow_data()?)?;
    require_keys_eq!(*a.lp_mint.owner, token::ID, BeamError::InvalidMigrationResult);
    require!(lp_mint.supply == plan.user_lp && lp_mint.mint_authority == Some(p.authority).into(), BeamError::InvalidMigrationResult);
    let lp = token_state(&a.authority_lp.to_account_info())?;
    require!(lp.mint == p.lp_mint && lp.owner == authority_info.key() && lp.amount == plan.user_lp, BeamError::InvalidMigrationResult);
    if create_pool_fee > 0 {
        let fee_after = token_state(&a.create_pool_fee_destination.to_account_info())?.amount;
        require!(fee_after == fee_dest_before.checked_add(create_pool_fee).ok_or(PumpError::Overflow)?, BeamError::InvalidMigrationResult);
    }

    // Raydium must have charged exactly the planned setup cost, no more and no less.
    let charged = authority_lamports_before
        .checked_add(setup_budget)
        .and_then(|funded| funded.checked_sub(authority_info.lamports()))
        .ok_or(BeamError::InvalidMigrationResult)?;
    require!(charged == required_setup, BeamError::InvalidMigrationResult);

    let refund = authority_info.lamports().saturating_sub(authority_lamports_before).min(setup_budget);
    if refund > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new_with_signer(a.system_program.to_account_info(), anchor_lang::system_program::Transfer { from: authority_info.clone(), to: a.user.to_account_info() }, &[authority_seeds]),
            refund,
        )?;
    }

    // Direct lamport moves come after the last CPI: the runtime rejects a CPI while
    // the caller's accounts are unbalanced. The inherited fee keeps its recipient,
    // and the curve must still cover rent and fee claims.
    checked_move(&curve_info, &a.withdraw_authority.to_account_info(), plan.migration_fee)?;
    require!(curve_info.lamports() >= plan.protected_floor, BeamError::InvalidMigrationBalance);

    let receipt = &mut ctx.accounts.receipt;
    receipt.mint = mint;
    receipt.pool = p.amm;
    receipt.lp_mint = p.lp_mint;
    receipt.coin_amount = plan.coin_amount;
    receipt.pc_amount = plan.pc_amount;
    receipt.migration_fee = plan.migration_fee;
    receipt.create_pool_fee = create_pool_fee;
    receipt.lp_amount = lp.amount;
    receipt.setup_spent = setup_budget - refund;
    receipt.open_time = open_time;
    receipt.lp_disposition = LP_HELD_UNRESOLVED;
    let curve = &mut ctx.accounts.bonding_curve;
    curve.real_quote_reserves = 0;
    curve.real_token_reserves = 0;
    curve.virtual_quote_reserves = 0;
    curve.virtual_token_reserves = 0;
    emit!(BeamAmmV4Migration { mint, pool: p.amm, coin_amount: plan.coin_amount, pc_amount: plan.pc_amount, migration_fee: plan.migration_fee, create_pool_fee, lp_amount: lp.amount });
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
    fn setup_cost_is_the_fee_plus_rent_for_the_six_created_accounts() {
        let rent = Rent::default();
        let rent_only = setup_cost(0, &rent).unwrap();
        assert_eq!(
            rent_only,
            rent.minimum_balance(752) + rent.minimum_balance(2208) + rent.minimum_balance(82) + 3 * rent.minimum_balance(165)
        );
        assert_eq!(setup_cost(25_000_000, &rent).unwrap(), rent_only + 25_000_000);
        assert!(setup_cost(u64::MAX, &rent).is_err());
    }

    #[test]
    fn lp_matches_raydium_integer_sqrt_minus_locked_minimum() {
        // sqrt(10^12 * 10^12) = 10^12; 10^6 is locked for six decimals.
        assert_eq!(expected_user_lp(1_000_000_000_000, 1_000_000_000_000, 6).unwrap(), 1_000_000_000_000 - 1_000_000);
        assert!(expected_user_lp(1, 1, 6).is_err());
        assert!(expected_user_lp(u64::MAX, u64::MAX, 6).is_ok());
    }

    #[test]
    fn plan_preserves_fee_liabilities_and_charges_the_migration_fee_once() {
        // 85 SOL of quote, 1 SOL migration fee, 3 + 2 SOL of fee claims, 1 SOL rent.
        let p = migration_plan(793_100_000_000_000, 1_000_000_000_000_000, 85_000_000_000, 500, 1_000_000_000, 100_000_000_000, 1_000_000, 3_000, 2_000, 6).unwrap();
        assert_eq!(p.pc_amount, 85_000_000_500 - 1_000_000_000);
        assert_eq!(p.protected_floor, 1_000_000 + 3_000 + 2_000);
        // Curve cannot cover recorded quote plus liabilities.
        assert!(migration_plan(1, 1, 85_000_000_000, 0, 0, 85_000_000_000, 1_000_000, 0, 0, 0).is_err());
        // More vault tokens than total supply, or an empty vault, never plan.
        assert!(migration_plan(2, 1, 1, 0, 0, u64::MAX, 0, 0, 0, 0).is_err());
        assert!(migration_plan(0, 1, 1, 0, 0, u64::MAX, 0, 0, 0, 0).is_err());
        // Migration fee larger than the quote leaves no pool quote.
        assert!(migration_plan(1_000_000_000, 1_000_000_000, 10, 0, 11, u64::MAX, 0, 0, 0, 0).is_err());
    }
}
