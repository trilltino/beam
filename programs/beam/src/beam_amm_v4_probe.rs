//! Host-only AMM v4 CPI diagnostic. This is not a migration instruction.
//!
//! It proves that Beam can sign the pinned 19-account Initialize2 CPI and
//! inspect its immediate result in a local bank. Every path returns an error,
//! including after a successful CPI and snapshot, so Solana rolls the whole
//! transaction back. The funded variant exercises curve-to-source preparation
//! and a current-Global migration fee, but neither settles funds nor disposes
//! LP in a committed transaction. Do not use real funds.

// A feature-enabled SBF build must never expose a callable diagnostic that
// consumes transaction fees for a guaranteed failure.
#[cfg(target_os = "solana")]
compile_error!("amm-v4-probe is a host-only diagnostic and cannot be built for Solana");

use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::AccountMeta,
        program::{invoke, invoke_signed},
        program_option::COption,
        program_pack::Pack,
    },
};
use anchor_spl::token::{
    self,
    spl_token::{
        native_mint,
        state::{Account as SplAccount, AccountState},
    },
    Mint, Token, TokenAccount, TransferChecked,
};

use crate::{
    beam_amm_v4::{
        initialize2_instruction, preflight_native_sol_initialize2, user_lp_ata,
        validate_create_pool_fee_account, Initialize2Args, DEVNET_PROGRAM_ID, MAINNET_PROGRAM_ID,
        SPL_TOKEN_PROGRAM_ID,
    },
    beam_amm_v4_funding::{self, BaseMintProgram, FundingInputs, QuoteAsset},
    beam_amm_v4_settlement::expected_keys,
    beam_amm_v4_snapshot::validate_post_initialize2,
    beam_policy::BeamError,
    trade_types::BondingCurve,
    Global,
};

#[cfg(feature = "raydium-devnet")]
const AMM_ID: Pubkey = DEVNET_PROGRAM_ID;
#[cfg(not(feature = "raydium-devnet"))]
const AMM_ID: Pubkey = MAINNET_PROGRAM_ID;

// Diagnostic upper bound only. It is stricter than unlimited, and the actual
// current fee is still read from the canonical Raydium config immediately
// before CPI. This is not a production fee policy or spending authorization.
const PROBE_CREATE_FEE_CAP_LAMPORTS: u64 = 1_000_000_000;
const PROBE_SETUP_RENT_CAP_LAMPORTS: u64 = 100_000_000;
const PROBE_SETUP_BUDGET_CAP_LAMPORTS: u64 = 150_000_000;

#[derive(Accounts)]
pub struct ProbeAmmV4<'info> {
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    pub authority: Signer<'info>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(seeds = [b"bonding-curve", mint.key().as_ref()], bump)]
    pub bonding_curve: Box<Account<'info, BondingCurve>>,
    /// CHECK: constrained to Beam's System-owned, empty signer PDA below.
    #[account(mut, seeds = [b"pool-authority", mint.key().as_ref()], bump, owner = anchor_lang::system_program::ID)]
    pub pool_authority: UncheckedAccount<'info>,
    /// CHECK: fixed executable AMM v4 program; no caller-selected venue.
    #[account(address = AMM_ID, executable)]
    pub amm_program: UncheckedAccount<'info>,
}

pub fn execute<'info>(
    ctx: Context<'_, '_, '_, 'info, ProbeAmmV4<'info>>,
    open_time: u64,
) -> Result<()> {
    let a = &ctx.accounts;
    let infos = ctx.remaining_accounts;
    require!(infos.len() == 19, BeamError::InvalidRaydiumAccount);
    require!(
        a.bonding_curve.complete
            && a.bonding_curve.real_token_reserves == 0
            && !a.bonding_curve.is_mayhem_mode
            && a.bonding_curve.quote_mint == Pubkey::default(),
        BeamError::MigrationNotEnabled
    );
    require!(
        a.pool_authority.data_is_empty(),
        BeamError::InvalidRaydiumAccount
    );

    let mint = a.mint.key();
    let (market_seed, _) =
        Pubkey::find_program_address(&[b"amm-v4-market", mint.as_ref()], &crate::ID);
    require_keys_eq!(
        infos[14].key(),
        market_seed,
        BeamError::InvalidRaydiumAccount
    );
    require_keys_eq!(
        infos[15].key(),
        a.pool_authority.key(),
        BeamError::InvalidRaydiumAccount
    );
    require_keys_eq!(infos[7].key(), mint, BeamError::InvalidRaydiumAccount);

    // A donated or precreated pool-related account must never be adopted by a
    // later real migration. Source ATAs 16/17 and the fee destination 13 are
    // deliberately excluded: they must already contain SPL token state.
    for index in [4, 6, 9, 10, 11, 18] {
        let info = &infos[index];
        require!(
            info.owner == &anchor_lang::system_program::ID
                && info.data_is_empty()
                && info.lamports() == 0,
            BeamError::InvalidRaydiumAccount
        );
    }
    for (index, info) in infos.iter().enumerate() {
        require!(
            !infos[index + 1..].iter().any(|other| other.key == info.key),
            BeamError::InvalidRaydiumAccount
        );
    }

    let base_source = SplAccount::unpack(&infos[16].try_borrow_data()?)
        .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let quote_source = SplAccount::unpack(&infos[17].try_borrow_data()?)
        .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let args = Initialize2Args {
        open_time,
        init_coin_amount: base_source.amount,
        init_pc_amount: quote_source.amount,
    };
    let program = a.amm_program.to_account_info();
    let accounts = preflight_native_sol_initialize2(
        &program,
        infos,
        &mint,
        args,
        PROBE_CREATE_FEE_CAP_LAMPORTS,
    )
    .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let ix = initialize2_instruction(AMM_ID, &accounts, args)
        .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let mut cpi_infos = infos.to_vec();
    cpi_infos.push(program.clone());
    let bump = [ctx.bumps.pool_authority];
    let signer_seeds: &[&[u8]] = &[b"pool-authority", mint.as_ref(), &bump];
    invoke_signed(&ix, &cpi_infos, &[signer_seeds])?;

    let clock = Clock::get()?.unix_timestamp;
    validate_post_initialize2(&program, infos, &mint, &market_seed, args, clock)
        .map_err(|_| error!(BeamError::InvalidMigrationResult))?;

    // No successful path in any host feature combination. This rolls back
    // Raydium CPI effects as well as Beam's own writes in the same transaction.
    err!(BeamError::MigrationNotEnabled)
}

/// Host-only diagnostic account ABI. All value accounts are authenticated by
/// Anchor and by the explicit cross-role checks in `execute_funded`. The 19
/// remaining accounts retain the pinned AMM v4 Initialize2 order.
#[derive(Accounts)]
pub struct ProbeAmmV4Funded<'info> {
    #[account(seeds = [b"global"], bump, has_one = authority)]
    pub global: Box<Account<'info, Global>>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, seeds = [b"bonding-curve", mint.key().as_ref()], bump)]
    pub bonding_curve: Box<Account<'info, BondingCurve>>,
    #[account(mut)]
    pub curve_base_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: constrained to Beam's System-owned signer PDA.
    #[account(mut, seeds = [b"pool-authority", mint.key().as_ref()], bump, owner = anchor_lang::system_program::ID)]
    pub pool_authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub base_source: Box<Account<'info, TokenAccount>>,
    #[account(address = native_mint::ID)]
    pub wsol_mint: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub wsol_source: Box<Account<'info, TokenAccount>>,
    /// CHECK: current Global recipient, mutable by admin and not launch-pinned.
    #[account(mut, address = global.withdraw_authority)]
    pub migration_fee_recipient: UncheckedAccount<'info>,
    /// CHECK: fixed executable AMM v4 program.
    #[account(address = AMM_ID, executable)]
    pub amm_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

fn clean_token(
    info: &AccountInfo<'_>,
    mint: &Pubkey,
    authority: Option<&Pubkey>,
    native: bool,
) -> Result<SplAccount> {
    require_keys_eq!(
        *info.owner,
        SPL_TOKEN_PROGRAM_ID,
        BeamError::InvalidRaydiumAccount
    );
    let token = SplAccount::unpack(&info.try_borrow_data()?)
        .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    require!(
        token.state == AccountState::Initialized
            && token.mint == *mint
            && authority.is_none_or(|owner| token.owner == *owner)
            && token.delegate == COption::None
            && token.delegated_amount == 0
            && token.close_authority == COption::None
            && token.is_native.is_some() == native,
        BeamError::InvalidRaydiumAccount
    );
    if native {
        let COption::Some(reserve) = token.is_native else {
            return err!(BeamError::InvalidRaydiumAccount);
        };
        require!(
            reserve.checked_add(token.amount) == Some(info.lamports()),
            BeamError::InvalidMigrationBalance
        );
    }
    Ok(token)
}

fn move_curve_lamports(from: &AccountInfo<'_>, to: &AccountInfo<'_>, amount: u64) -> Result<()> {
    require!(from.key != to.key, BeamError::InvalidRaydiumAccount);
    let left = from
        .lamports()
        .checked_sub(amount)
        .ok_or(BeamError::InvalidMigrationBalance)?;
    let right = to
        .lamports()
        .checked_add(amount)
        .ok_or(BeamError::InvalidMigrationBalance)?;
    **from.try_borrow_mut_lamports()? = left;
    **to.try_borrow_mut_lamports()? = right;
    Ok(())
}

/// Exercise real Beam-owned reserve movement but NEVER commit it. In every
/// feature combination this diagnostic returns an error after its checks.
/// Current Global settings are mutable, so these are only test-time premises,
/// not durable migration policy or proof of donation-free launch balances.
pub fn execute_funded<'info>(
    ctx: Context<'_, '_, '_, 'info, ProbeAmmV4Funded<'info>>,
    open_time: u64,
    setup_budget: u64,
    setup_rent_cap: u64,
) -> Result<()> {
    let a = &ctx.accounts;
    let infos = ctx.remaining_accounts;
    require!(infos.len() == 19, BeamError::InvalidRaydiumAccount);
    require!(
        setup_budget > 0
            && setup_budget <= PROBE_SETUP_BUDGET_CAP_LAMPORTS
            && setup_rent_cap <= PROBE_SETUP_RENT_CAP_LAMPORTS,
        BeamError::InvalidMigrationBalance
    );
    let curve = a.bonding_curve.to_account_info();
    let mint = a.mint.key();
    let recipient = a.migration_fee_recipient.to_account_info();
    let source = a.base_source.to_account_info();
    let wsol_source = a.wsol_source.to_account_info();
    let base_vault = a.curve_base_vault.to_account_info();
    let pool_authority = a.pool_authority.to_account_info();
    let program = a.amm_program.to_account_info();
    let (market_seed, _) =
        Pubkey::find_program_address(&[b"amm-v4-market", mint.as_ref()], &crate::ID);

    require!(
        a.bonding_curve.complete
            && a.bonding_curve.real_token_reserves == 0
            && a.bonding_curve.virtual_quote_reserves > 0
            && !a.bonding_curve.is_mayhem_mode
            && a.bonding_curve.quote_mint == Pubkey::default()
            && pool_authority.data_is_empty(),
        BeamError::MigrationNotEnabled
    );
    let typed = [
        a.global.key(),
        a.authority.key(),
        mint,
        curve.key(),
        base_vault.key(),
        pool_authority.key(),
        source.key(),
        a.wsol_mint.key(),
        wsol_source.key(),
        recipient.key(),
        program.key(),
        a.token_program.key(),
        a.system_program.key(),
    ];
    for (i, key) in typed.iter().enumerate() {
        require!(
            !typed[i + 1..].contains(key),
            BeamError::InvalidRaydiumAccount
        );
    }
    // Only these seven roles intentionally appear once in Beam's typed ABI
    // and again in the pinned Raydium 19-account ABI. In particular, the
    // migration recipient cannot alias config, pool, target, fee destination,
    // or any other CPI role even if its balance happens to match later.
    let allowed_equalities = [(2, 7), (5, 15), (6, 16), (7, 8), (8, 17), (11, 0), (12, 2)];
    for (typed_index, key) in typed.iter().enumerate() {
        for (raydium_index, info) in infos.iter().enumerate() {
            if key == info.key {
                require!(
                    allowed_equalities.contains(&(typed_index, raydium_index)),
                    BeamError::InvalidRaydiumAccount
                );
            }
        }
    }
    for (index, info) in infos.iter().enumerate() {
        require!(
            !infos[index + 1..].iter().any(|other| other.key == info.key),
            BeamError::InvalidRaydiumAccount
        );
    }
    require_keys_eq!(infos[7].key(), mint, BeamError::InvalidRaydiumAccount);
    require_keys_eq!(
        infos[8].key(),
        a.wsol_mint.key(),
        BeamError::InvalidRaydiumAccount
    );
    require_keys_eq!(
        infos[15].key(),
        pool_authority.key(),
        BeamError::InvalidRaydiumAccount
    );
    require_keys_eq!(
        infos[16].key(),
        source.key(),
        BeamError::InvalidRaydiumAccount
    );
    require_keys_eq!(
        infos[17].key(),
        wsol_source.key(),
        BeamError::InvalidRaydiumAccount
    );
    require_keys_eq!(
        infos[14].key(),
        market_seed,
        BeamError::InvalidRaydiumAccount
    );
    for index in [4, 6, 9, 10, 11, 18] {
        let info = &infos[index];
        require!(
            info.owner == &anchor_lang::system_program::ID
                && info.data_is_empty()
                && info.lamports() == 0,
            BeamError::InvalidRaydiumAccount
        );
    }

    let keys = expected_keys(&AMM_ID, &mint, &market_seed, curve.key(), recipient.key())
        .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    require!(
        keys.curve_base_vault == base_vault.key()
            && keys.setup_authority == pool_authority.key()
            && keys.base_source == source.key()
            && keys.wsol_source == wsol_source.key()
            && keys.base_vault == infos[9].key()
            && keys.wsol_vault == infos[10].key()
            && keys.lp_destination == infos[18].key()
            && keys.create_fee_destination == infos[13].key(),
        BeamError::InvalidRaydiumAccount
    );
    let base = clean_token(&base_vault, &mint, Some(&curve.key()), false)?.amount;
    require!(base > 0, BeamError::InvalidMigrationBalance);
    let expected_base = a
        .bonding_curve
        .token_total_supply
        .checked_sub(a.global.initial_real_token_reserves)
        .and_then(|value| value.checked_sub(a.bonding_curve.post_complete_base_out))
        .ok_or(BeamError::InvalidMigrationBalance)?;
    // This rejects a vault balance that exceeds the current reconstructed
    // entitlement. Global's initial reserve is mutable, so it is not a
    // production-strength proof of donor absence.
    require!(base == expected_base, BeamError::InvalidMigrationBalance);
    require!(
        clean_token(&source, &mint, Some(&pool_authority.key()), false)?.amount == 0,
        BeamError::InvalidRaydiumAccount
    );
    require!(
        clean_token(
            &wsol_source,
            &native_mint::ID,
            Some(&pool_authority.key()),
            true
        )?
        .amount
            == 0,
        BeamError::InvalidRaydiumAccount
    );
    let fee_before = clean_token(&infos[13], &native_mint::ID, None, true)?;
    let fee_lamports_before = infos[13].lamports();
    let recipient_before = recipient.lamports();
    let curve_before = curve.lamports();
    let setup_before = pool_authority.lamports();
    let current_fee =
        validate_create_pool_fee_account(&AMM_ID, &infos[12], PROBE_CREATE_FEE_CAP_LAMPORTS)
            .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let plan = beam_amm_v4_funding::plan(FundingInputs {
        quote_asset: QuoteAsset::NativeSol,
        base_mint_program: BaseMintProgram::ClassicSpl,
        base_tokens_to_pool: base,
        curve_lamports: curve_before,
        curve_rent_floor: Rent::get()?.minimum_balance(curve.data_len()),
        real_quote_reserves: a.bonding_curve.real_quote_reserves,
        post_complete_quote_in: a.bonding_curve.post_complete_quote_in,
        creator_fee_liability: a.bonding_curve.creator_fee,
        protocol_fee_liability: a.bonding_curve.protocol_fees,
        migration_fee: a.global.pool_migration_fee,
        caller_setup_budget: setup_budget,
        setup_authority_before: setup_before,
        current_create_pool_fee: current_fee,
        max_create_pool_fee: PROBE_CREATE_FEE_CAP_LAMPORTS,
        setup_rent_cap,
    })
    .map_err(|_| error!(BeamError::InvalidMigrationBalance))?;

    anchor_lang::system_program::transfer(
        CpiContext::new(
            a.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: a.authority.to_account_info(),
                to: pool_authority.clone(),
            },
        ),
        setup_budget,
    )?;
    let curve_bump = [ctx.bumps.bonding_curve];
    let curve_seeds: &[&[u8]] = &[b"bonding-curve", mint.as_ref(), &curve_bump];
    token::transfer_checked(
        CpiContext::new_with_signer(
            a.token_program.to_account_info(),
            TransferChecked {
                from: base_vault.clone(),
                mint: a.mint.to_account_info(),
                to: source.clone(),
                authority: curve.clone(),
            },
            &[curve_seeds],
        ),
        base,
        a.mint.decimals,
    )?;
    move_curve_lamports(&curve, &wsol_source, plan.pool_quote)?;
    // Include both sides of the direct Beam-owned lamport movement in this
    // CPI's account metas. The classic SPL processor reads the WSOL account;
    // Solana also needs the debited curve account to reconcile this inner
    // instruction's lamport sum. The retained native migration does likewise.
    let mut sync_ix =
        token::spl_token::instruction::sync_native(&a.token_program.key(), &wsol_source.key())?;
    sync_ix.accounts.push(AccountMeta::new(curve.key(), false));
    invoke(
        &sync_ix,
        &[
            wsol_source.clone(),
            curve.clone(),
            a.token_program.to_account_info(),
        ],
    )?;
    require!(
        clean_token(&base_vault, &mint, Some(&curve.key()), false)?.amount == 0
            && clean_token(&source, &mint, Some(&pool_authority.key()), false)?.amount == base
            && clean_token(
                &wsol_source,
                &native_mint::ID,
                Some(&pool_authority.key()),
                true
            )?
            .amount
                == plan.pool_quote
            && curve.lamports()
                == curve_before
                    .checked_sub(plan.pool_quote)
                    .ok_or(BeamError::InvalidMigrationBalance)?
            && pool_authority.lamports() == plan.setup_authority_after_funding,
        BeamError::InvalidMigrationBalance
    );

    let args = Initialize2Args {
        open_time,
        init_coin_amount: base,
        init_pc_amount: plan.pool_quote,
    };
    let accounts = preflight_native_sol_initialize2(
        &program,
        infos,
        &mint,
        args,
        PROBE_CREATE_FEE_CAP_LAMPORTS,
    )
    .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    require!(
        validate_create_pool_fee_account(&AMM_ID, &infos[12], PROBE_CREATE_FEE_CAP_LAMPORTS)
            .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?
            == current_fee,
        BeamError::InvalidRaydiumAccount
    );
    let ix = initialize2_instruction(AMM_ID, &accounts, args)
        .map_err(|_| error!(BeamError::InvalidRaydiumAccount))?;
    let mut cpi_infos = infos.to_vec();
    cpi_infos.push(program.clone());
    let authority_bump = [ctx.bumps.pool_authority];
    let authority_seeds: &[&[u8]] = &[b"pool-authority", mint.as_ref(), &authority_bump];
    invoke_signed(&ix, &cpi_infos, &[authority_seeds])?;
    validate_post_initialize2(
        &program,
        infos,
        &mint,
        &market_seed,
        args,
        Clock::get()?.unix_timestamp,
    )
    .map_err(|_| error!(BeamError::InvalidMigrationResult))?;

    let fee_after = clean_token(&infos[13], &native_mint::ID, None, true)?;
    require!(
        fee_after.amount.checked_sub(fee_before.amount) == Some(current_fee)
            && infos[13].lamports().checked_sub(fee_lamports_before) == Some(current_fee),
        BeamError::InvalidMigrationResult
    );
    move_curve_lamports(&curve, &recipient, plan.migration_fee_unrouted)?;
    require!(
        recipient.lamports().checked_sub(recipient_before) == Some(plan.migration_fee_unrouted)
            && curve.lamports() == plan.curve_after_minimum,
        BeamError::InvalidMigrationBalance
    );
    for index in [4, 6, 9, 10, 11, 18] {
        require!(
            infos[index].lamports() >= Rent::get()?.minimum_balance(infos[index].data_len()),
            BeamError::InvalidMigrationResult
        );
    }
    beam_amm_v4_funding::settle(plan, curve.lamports(), pool_authority.lamports())
        .map_err(|_| error!(BeamError::InvalidMigrationBalance))?;
    // A valid diagnostic still aborts. No receipt, LP disposition, or replay
    // policy is committed; transaction-payer fees remain outside this check.
    err!(BeamError::MigrationNotEnabled)
}
