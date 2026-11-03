//! Shared synthetic local-bank setup for the Beam trade runtime tests.
//! The inherited Pump fee program is a synthetic processor that returns
//! fixed test bps; nothing here is Beam or Pump fee policy.
#![allow(dead_code)]
use anchor_lang::{
    prelude::Pubkey, solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, program::set_return_data},
    AccountDeserialize, AccountSerialize, AnchorSerialize, InstructionData, ToAccountMetas,
};
use anchor_spl::{
    associated_token::{get_associated_token_address_with_program_id as ata, spl_associated_token_account},
    token_2022::spl_token_2022::{self, extension::StateWithExtensions, state::Account as TokenAccount},
};
use beam_program::{
    fee_math::fee, pump_fees::{accounts::FeeConfig, types::Fees},
    trade_types::BondingCurve, Global, GlobalVolumeAccumulator, OptionBool, PumpError, CreationOptionU64,
};
use anchor_lang::solana_program::program_pack::Pack;
use solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account, instruction::{Instruction, InstructionError}, rent::Rent,
    signature::{Keypair, Signer}, system_program, transaction::{Transaction, TransactionError},
};

pub const FEE_PROGRAM: Pubkey = beam_program::pump_fees::ID;
pub const MAYHEM: Pubkey = beam_program::mayhem::ID;
pub const PROTOCOL_BPS: u64 = 95;
pub const CREATOR_BPS: u64 = 30;
pub const BUYBACK_BPS: u64 = 2_000;
pub const INITIAL_VIRTUAL_TOKENS: u64 = 1_073_000_000_000_000;
pub const INITIAL_VIRTUAL_SOL: u64 = 30_000_000_000;
pub const INITIAL_REAL_TOKENS: u64 = 793_100_000_000_000;
pub const TOTAL_SUPPLY: u64 = 1_000_000_000_000_000;
pub const GET_FEES_WITH_QUOTE_MINT: [u8; 8] = [154, 237, 138, 92, 162, 2, 162, 187];

pub fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey { Pubkey::find_program_address(seeds, program).0 }
pub fn global_pda() -> Pubkey { pda(&[b"global"], &beam_program::ID) }
pub fn curve_pda(mint: &Pubkey) -> Pubkey { pda(&[b"bonding-curve", mint.as_ref()], &beam_program::ID) }
pub fn creator_vault(creator: &Pubkey) -> Pubkey { pda(&[b"creator-vault", creator.as_ref()], &beam_program::ID) }
pub fn event_authority() -> Pubkey { pda(&[b"__event_authority"], &beam_program::ID) }
pub fn fee_config_pda() -> Pubkey { pda(&[b"fee_config", beam_program::ID.as_ref()], &FEE_PROGRAM) }

pub fn beam_processor<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>], data: &[u8]) -> ProgramResult {
    // SAFETY: ProgramTest invokes the processor synchronously and neither side
    // retains the slice; Anchor's entrypoint requires the shared lifetime.
    let accounts: &'a [AccountInfo<'a>] = unsafe { std::mem::transmute(accounts) };
    beam_program::entry(program_id, accounts, data)
}

/// Synthetic stand-in for the inherited fee program's `get_fees_with_quote_mint`.
pub fn fee_processor(_program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    assert_eq!(&data[..8], &GET_FEES_WITH_QUOTE_MINT, "unexpected fee instruction");
    assert_eq!(data.len(), 8 + 1 + 16 + 32, "bool, u128 market cap, quote mint");
    assert_eq!(data[8], 1, "legacy trades query with is_pump_pool = true");
    assert!(u128::from_le_bytes(data[9..25].try_into().unwrap()) > 0, "market cap is positive");
    assert_eq!(&data[25..57], Pubkey::default().as_ref(), "native SOL quote mint");
    assert_eq!(*accounts[0].key, fee_config_pda());
    assert_eq!(*accounts[1].key, beam_program::ID);
    let fees = Fees { lp_fee_bps: 0, protocol_fee_bps: PROTOCOL_BPS, creator_fee_bps: CREATOR_BPS };
    set_return_data(&fees.try_to_vec().unwrap());
    Ok(())
}

pub fn owned(data: Vec<u8>, owner: Pubkey) -> Account {
    Account { lamports: Rent::default().minimum_balance(data.len()), data, owner, executable: false, rent_epoch: 0 }
}
pub fn wallet(lamports: u64) -> Account {
    Account { lamports, data: vec![], owner: system_program::ID, executable: false, rent_epoch: 0 }
}

pub struct Bank {
    pub ctx: ProgramTestContext,
    pub user: Keypair,
    pub creator: Pubkey,
    pub fee_recipient: Pubkey,
    pub buyback: Pubkey,
    pub stranger: Pubkey,
    pub mint: Keypair,
}

pub async fn bank() -> Bank {
    let user = Keypair::new();
    let creator = Pubkey::new_unique();
    let fee_recipient = Pubkey::new_unique();
    let buyback = Pubkey::new_unique();
    let stranger = Pubkey::new_unique();

    let zeroed = vec![0u8; 4_096];
    let mut global = Global::try_deserialize_unchecked(&mut &zeroed[..]).unwrap();
    global.initialized = true;
    global.authority = Pubkey::new_unique();
    global.fee_recipient = fee_recipient;
    global.initial_virtual_token_reserves = INITIAL_VIRTUAL_TOKENS;
    global.initial_virtual_sol_reserves = INITIAL_VIRTUAL_SOL;
    global.initial_real_token_reserves = INITIAL_REAL_TOKENS;
    global.token_total_supply = TOTAL_SUPPLY;
    global.create_v2_enabled = true;
    global.whitelist_pda = Pubkey::new_unique();
    global.buyback_fee_recipients = [buyback; 8];
    global.buyback_basis_points = BUYBACK_BPS;
    let mut global_data = Vec::new();
    global.try_serialize(&mut global_data).unwrap();

    let zeroed_volume = vec![0u8; 1_024];
    let volume = GlobalVolumeAccumulator::try_deserialize_unchecked(&mut &zeroed_volume[..]).unwrap();
    let mut volume_data = Vec::new();
    volume.try_serialize(&mut volume_data).unwrap();

    let empty = Fees { lp_fee_bps: 0, protocol_fee_bps: 0, creator_fee_bps: 0 };
    let config = FeeConfig {
        bump: 0, admin: Pubkey::default(), flat_fees: empty.clone(),
        fee_tiers: vec![], stable_fee_tiers: vec![], exotic_flat_fees: empty,
    };
    let mut config_data = Vec::new();
    config.try_serialize(&mut config_data).unwrap();

    let mut test = ProgramTest::new("beam_program", beam_program::ID, processor!(beam_processor));
    test.add_program("synthetic_fee_program", FEE_PROGRAM, processor!(fee_processor));
    test.add_account(global_pda(), owned(global_data, beam_program::ID));
    test.add_account(pda(&[b"global_volume_accumulator"], &beam_program::ID), owned(volume_data, beam_program::ID));
    test.add_account(fee_config_pda(), owned(config_data, FEE_PROGRAM));
    // V2 handlers take the WSOL mint as `quote_mint` for native-SOL curves.
    let mut wsol = vec![0u8; anchor_spl::token::spl_token::state::Mint::LEN];
    anchor_spl::token::spl_token::state::Mint {
        mint_authority: anchor_lang::solana_program::program_option::COption::None, supply: 0, decimals: 9,
        is_initialized: true, freeze_authority: anchor_lang::solana_program::program_option::COption::None,
    }.pack_into_slice(&mut wsol);
    test.add_account(anchor_spl::token::spl_token::native_mint::ID, owned(wsol, anchor_spl::token::ID));
    test.add_account(user.pubkey(), wallet(100_000_000_000));
    // Pre-funded so that small fee transfers never leave them below rent.
    test.add_account(fee_recipient, wallet(1_000_000_000));
    test.add_account(buyback, wallet(1_000_000_000));
    test.add_account(stranger, wallet(1_000_000_000));
    Bank { ctx: test.start_with_context().await, user, creator, fee_recipient, buyback, stranger, mint: Keypair::new() }
}

impl Bank {
    pub async fn send(&mut self, instructions: &[Instruction], extra: &[&Keypair]) -> Result<(), BanksClientError> {
        let blockhash = self.ctx.banks_client.get_latest_blockhash().await.unwrap();
        let mut signers: Vec<&Keypair> = vec![&self.ctx.payer, &self.user];
        signers.extend_from_slice(extra);
        let tx = Transaction::new_signed_with_payer(instructions, Some(&self.ctx.payer.pubkey()), &signers, blockhash);
        self.ctx.banks_client.process_transaction(tx).await
    }
    pub async fn lamports(&mut self, key: Pubkey) -> u64 {
        self.ctx.banks_client.get_account(key).await.unwrap().map_or(0, |a| a.lamports)
    }
    pub async fn curve(&mut self) -> BondingCurve {
        let data = self.ctx.banks_client.get_account(curve_pda(&self.mint.pubkey())).await.unwrap().unwrap().data;
        BondingCurve::try_deserialize(&mut &data[..]).unwrap()
    }
    pub async fn token_balance(&mut self, owner: &Pubkey) -> u64 {
        let key = ata(owner, &self.mint.pubkey(), &spl_token_2022::ID);
        let data = self.ctx.banks_client.get_account(key).await.unwrap().unwrap().data;
        StateWithExtensions::<TokenAccount>::unpack(&data).unwrap().base.amount
    }

    pub fn create_v2(&self) -> Instruction {
        let mint = self.mint.pubkey();
        let curve = curve_pda(&mint);
        let accounts = beam_program::accounts::CreateV2 {
            mint, mint_authority: pda(&[b"mint-authority"], &beam_program::ID), bonding_curve: curve,
            associated_bonding_curve: ata(&curve, &mint, &spl_token_2022::ID), global: global_pda(),
            user: self.user.pubkey(), system_program: system_program::ID, token_program: spl_token_2022::ID,
            associated_token_program: spl_associated_token_account::ID, mayhem_program_id: MAYHEM,
            global_params: pda(&[b"global-params"], &MAYHEM), sol_vault: pda(&[b"sol-vault"], &MAYHEM),
            mayhem_state: pda(&[b"mayhem-state", mint.as_ref()], &MAYHEM),
            mayhem_token_vault: Pubkey::new_unique(), event_authority: event_authority(), program: beam_program::ID,
        };
        Instruction {
            program_id: beam_program::ID,
            accounts: accounts.to_account_metas(None),
            data: beam_program::instruction::CreateV2 {
                name: "Beam Runtime".into(), symbol: "BRT".into(), uri: "https://example.invalid/brt.json".into(),
                creator: self.creator, is_mayhem_mode: false, is_cashback_enabled: OptionBool(false),
                creator_fee_bps: CreationOptionU64(0), is_holder_reward: OptionBool(false),
            }.data(),
        }
    }

    pub fn create_user_ata(&self) -> Instruction {
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            &self.user.pubkey(), &self.user.pubkey(), &self.mint.pubkey(), &spl_token_2022::ID,
        )
    }
}

pub fn custom_error(result: Result<(), BanksClientError>) -> u32 {
    match result.expect_err("transaction must fail").unwrap() {
        TransactionError::InstructionError(_, InstructionError::Custom(code)) => code,
        other => panic!("unexpected error {other:?}"),
    }
}
pub fn code(error: PumpError) -> u32 { error.into() }

pub struct Expected { pub cost: u64, pub protocol: u64, pub creator: u64, pub buyback: u64 }
pub fn expected_fees(quote: u64) -> Expected {
    let protocol = fee(quote, PROTOCOL_BPS).unwrap();
    let creator = fee(quote, CREATOR_BPS).unwrap();
    let buyback = u64::try_from(u128::from(BUYBACK_BPS) * u128::from(protocol) / 10_000).unwrap();
    Expected { cost: quote, protocol, creator, buyback }
}
