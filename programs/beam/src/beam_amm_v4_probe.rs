//! Host-only AMM v4 CPI diagnostic. This is not a migration instruction.
//!
//! It proves that Beam can sign the pinned 19-account Initialize2 CPI and
//! inspect its immediate result in a local bank. Every path returns an error,
//! including after a successful CPI and snapshot, so Solana rolls the whole
//! transaction back. No curve funding, migration fee, LP disposition, or
//! post-graduation revenue policy is implemented here. Do not use real funds.

// A feature-enabled SBF build must never expose a callable diagnostic that
// consumes transaction fees for a guaranteed failure.
#[cfg(target_os = "solana")]
compile_error!("amm-v4-probe is a host-only diagnostic and cannot be built for Solana");

use anchor_lang::{
    prelude::*,
    solana_program::{program::invoke_signed, program_pack::Pack},
};
use anchor_spl::token::{spl_token::state::Account as SplAccount, Mint};

use crate::{
    beam_amm_v4::{
        initialize2_instruction, preflight_native_sol_initialize2, Initialize2Args,
        DEVNET_PROGRAM_ID, MAINNET_PROGRAM_ID,
    },
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
