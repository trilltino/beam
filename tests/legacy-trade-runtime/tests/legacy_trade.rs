//! Executes Beam's native-SOL `create_v2`, legacy `buy` and legacy `sell`
//! handlers in a synthetic local bank and checks every curve, lamport and
//! token delta against the program's own pure quote and fee functions.
//!
//! The inherited Pump fee program is replaced by a synthetic processor that
//! returns fixed basis points. Those bps are test inputs, not Beam or Pump
//! fee policy. Nothing here touches a live network, wallet or fee program.
mod common;

use common::*;
use anchor_lang::{prelude::Pubkey, InstructionData, ToAccountMetas};
use anchor_spl::{
    associated_token::get_associated_token_address_with_program_id as ata,
    token_2022::spl_token_2022::{self, extension::StateWithExtensions, state::Mint},
};
use beam_program::{buy_quote, OptionBool, PumpError};
use solana_sdk::{
    instruction::{AccountMeta, Instruction}, rent::Rent, signature::Signer, system_program,
};

impl Bank {
    /// Base metas from Anchor, plus the two remaining accounts the handlers
    /// read: `[bonding-curve-v2 PDA, buyback fee recipient]`.
    fn trade_metas(&self, buy: bool, fee_recipient: Pubkey, vault_creator: Pubkey, remaining: bool) -> Vec<AccountMeta> {
        let mint = self.mint.pubkey();
        let curve = curve_pda(&mint);
        let user = self.user.pubkey();
        let mut metas = if buy {
            beam_program::accounts::LegacyBuy {
                global: global_pda(), fee_recipient, mint, bonding_curve: curve,
                associated_bonding_curve: ata(&curve, &mint, &spl_token_2022::ID),
                associated_user: ata(&user, &mint, &spl_token_2022::ID), user,
                system_program: system_program::ID, token_program: spl_token_2022::ID,
                creator_vault: creator_vault(&vault_creator), event_authority: event_authority(), program: beam_program::ID,
                global_volume_accumulator: pda(&[b"global_volume_accumulator"], &beam_program::ID),
                user_volume_accumulator: pda(&[b"user_volume_accumulator", user.as_ref()], &beam_program::ID),
                fee_config: fee_config_pda(), fee_program: FEE_PROGRAM,
            }.to_account_metas(None)
        } else {
            beam_program::accounts::LegacySell {
                global: global_pda(), fee_recipient, mint, bonding_curve: curve,
                associated_bonding_curve: ata(&curve, &mint, &spl_token_2022::ID),
                associated_user: ata(&user, &mint, &spl_token_2022::ID), user,
                system_program: system_program::ID, creator_vault: creator_vault(&vault_creator),
                token_program: spl_token_2022::ID, event_authority: event_authority(), program: beam_program::ID,
                fee_config: fee_config_pda(), fee_program: FEE_PROGRAM,
            }.to_account_metas(None)
        };
        if remaining {
            metas.push(AccountMeta::new_readonly(pda(&[b"bonding-curve-v2", mint.as_ref()], &beam_program::ID), false));
            metas.push(AccountMeta::new(self.buyback, false));
        }
        metas
    }

    fn buy(&self, amount: u64, max_sol_cost: u64, partial_fill: bool) -> Instruction {
        self.buy_with(amount, max_sol_cost, partial_fill, self.fee_recipient, self.creator, true)
    }
    fn buy_with(&self, amount: u64, max_sol_cost: u64, partial_fill: bool, fee_recipient: Pubkey, vault_creator: Pubkey, remaining: bool) -> Instruction {
        Instruction {
            program_id: beam_program::ID,
            accounts: self.trade_metas(true, fee_recipient, vault_creator, remaining),
            data: beam_program::instruction::Buy {
                amount, max_sol_cost, track_volume: OptionBool(false), partial_fill: OptionBool(partial_fill),
            }.data(),
        }
    }
    fn sell(&self, amount: u64, min_sol_output: u64) -> Instruction {
        Instruction {
            program_id: beam_program::ID,
            accounts: self.trade_metas(false, self.fee_recipient, self.creator, true),
            data: beam_program::instruction::Sell { amount, min_sol_output }.data(),
        }
    }
}

#[tokio::test]
async fn create_buy_sell_match_pure_quote_and_fee_math() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2()], &[&mint]).await.expect("create_v2");

    let curve = b.curve().await;
    assert_eq!((curve.virtual_token_reserves, curve.virtual_quote_reserves), (INITIAL_VIRTUAL_TOKENS, INITIAL_VIRTUAL_SOL));
    assert_eq!((curve.real_token_reserves, curve.real_quote_reserves), (INITIAL_REAL_TOKENS, 0));
    assert_eq!((curve.creator, curve.quote_mint, curve.complete), (b.creator, Pubkey::default(), false));
    assert_eq!(b.token_balance(&curve_pda(&b.mint.pubkey())).await, TOTAL_SUPPLY);
    let mint_data = b.ctx.banks_client.get_account(b.mint.pubkey()).await.unwrap().unwrap().data;
    let mint_state = StateWithExtensions::<Mint>::unpack(&mint_data).unwrap().base;
    assert_eq!((mint_state.supply, mint_state.decimals), (TOTAL_SUPPLY, 6));
    assert!(mint_state.mint_authority.is_none(), "mint authority is revoked after creation");

    // ---- buy: max_sol_cost exactly at the bound succeeds, one lamport less fails.
    let amount = 35_000_000_000_000;
    let quote = u64::try_from(buy_quote(&curve, amount).unwrap()).unwrap();
    let fees = expected_fees(quote);
    let total = fees.cost + fees.protocol + fees.creator;
    let ata_ix = b.create_user_ata();
    b.send(&[ata_ix.clone()], &[]).await.unwrap();
    assert_eq!(custom_error(b.send(&[b.buy(amount, total - 1, false)], &[]).await), code(PumpError::TooMuchSolRequired));

    let curve_key = curve_pda(&b.mint.pubkey());
    let vault = creator_vault(&b.creator);
    let before = (b.lamports(curve_key).await, b.lamports(b.fee_recipient).await, b.lamports(b.buyback).await, b.lamports(vault).await);
    b.send(&[b.buy(amount, total, false)], &[]).await.expect("buy at exact bound");
    let after = (b.lamports(curve_key).await, b.lamports(b.fee_recipient).await, b.lamports(b.buyback).await, b.lamports(vault).await);
    assert_eq!(after.0 - before.0, fees.cost, "curve receives the quote");
    assert_eq!(after.1 - before.1, fees.protocol - fees.buyback, "fee recipient receives the protocol remainder");
    assert_eq!(after.2 - before.2, fees.buyback, "buyback recipient receives its share");
    assert_eq!(after.3 - before.3, Rent::default().minimum_balance(0) + fees.creator, "creator vault: rent top-up then creator fee");
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount);
    let bought = b.curve().await;
    assert_eq!(bought.virtual_token_reserves, curve.virtual_token_reserves - amount);
    assert_eq!(bought.real_token_reserves, curve.real_token_reserves - amount);
    assert_eq!(bought.virtual_quote_reserves, curve.virtual_quote_reserves + fees.cost);
    assert_eq!(bought.real_quote_reserves, fees.cost);

    // ---- sell: min_sol_output exactly at the bound succeeds, one lamport more fails.
    let sold = amount / 2;
    let gross = u64::try_from(bought.sell_quote(sold).unwrap()).unwrap();
    let fees = expected_fees(gross);
    let net = gross - fees.protocol - fees.creator;
    assert_eq!(custom_error(b.send(&[b.sell(sold, net + 1)], &[]).await), code(PumpError::TooLittleSolReceived));
    let user_before = b.lamports(b.user.pubkey()).await;
    let before = (b.lamports(curve_key).await, b.lamports(b.fee_recipient).await, b.lamports(b.buyback).await, b.lamports(vault).await);
    b.send(&[b.sell(sold, net)], &[]).await.expect("sell at exact bound");
    let after = (b.lamports(curve_key).await, b.lamports(b.fee_recipient).await, b.lamports(b.buyback).await, b.lamports(vault).await);
    assert_eq!(b.lamports(b.user.pubkey()).await - user_before, net, "user receives gross minus fees");
    assert_eq!(before.0 - after.0, gross, "curve pays the gross quote");
    assert_eq!(after.1 - before.1, fees.protocol - fees.buyback);
    assert_eq!(after.2 - before.2, fees.buyback);
    assert_eq!(after.3 - before.3, fees.creator);
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount - sold);
    let after_sell = b.curve().await;
    assert_eq!(after_sell.virtual_token_reserves, bought.virtual_token_reserves + sold);
    assert_eq!(after_sell.virtual_quote_reserves, bought.virtual_quote_reserves - gross);
    assert_eq!(after_sell.real_quote_reserves, bought.real_quote_reserves - gross);
}

#[tokio::test]
async fn partial_fill_clamps_to_real_reserves_and_completes_the_curve() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    let curve = b.curve().await;
    let quote = u64::try_from(buy_quote(&curve, curve.real_token_reserves).unwrap()).unwrap();
    let fees = expected_fees(quote);
    let total = fees.cost + fees.protocol + fees.creator;

    assert_eq!(custom_error(b.send(&[b.buy(INITIAL_REAL_TOKENS + 1, u64::MAX, false)], &[]).await), code(PumpError::NotEnoughTokensToBuy));
    b.send(&[b.buy(INITIAL_REAL_TOKENS + 1, total, true)], &[]).await.expect("partial fill buy");
    let done = b.curve().await;
    assert_eq!(b.token_balance(&b.user.pubkey()).await, INITIAL_REAL_TOKENS);
    assert_eq!(done.real_token_reserves, 0);
    assert!(done.complete, "curve completes when real token reserves reach zero");
    assert_eq!(custom_error(b.send(&[b.buy(1, u64::MAX, true)], &[]).await), code(PumpError::BondingCurveComplete));
    assert_eq!(custom_error(b.send(&[b.sell(1, 0)], &[]).await), code(PumpError::BondingCurveComplete));
}

#[tokio::test]
async fn trades_reject_missing_remaining_accounts_and_substituted_recipients() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    let (fee_recipient, creator, stranger) = (b.fee_recipient, b.creator, b.stranger);

    // The 16 base metas alone (what the SDK builder returns) are not executable.
    let base_only = b.buy_with(1_000_000, u64::MAX, false, fee_recipient, creator, false);
    assert_eq!(custom_error(b.send(&[base_only], &[]).await), code(PumpError::BuybackFeeRecipientMissing));
    let wrong_recipient = b.buy_with(1_000_000, u64::MAX, false, stranger, creator, true);
    assert_eq!(custom_error(b.send(&[wrong_recipient], &[]).await), code(PumpError::NotAuthorized));
    let wrong_vault = b.buy_with(1_000_000, u64::MAX, false, fee_recipient, stranger, true);
    let seeds: u32 = anchor_lang::error::ErrorCode::ConstraintSeeds.into();
    assert_eq!(custom_error(b.send(&[wrong_vault], &[]).await), seeds);
    assert_eq!(custom_error(b.send(&[b.buy(0, u64::MAX, false)], &[]).await), code(PumpError::BuyZeroAmount));
    assert_eq!(custom_error(b.send(&[b.sell(0, 0)], &[]).await), code(PumpError::SellZeroAmount));
}

// ---------------------------------------------------------------- creator fees

impl Bank {
    /// Signed only by the test payer: neither the creator nor the trader signs.
    async fn send_as_third_party(&mut self, instruction: Instruction) -> Result<(), solana_program_test::BanksClientError> {
        let blockhash = self.ctx.banks_client.get_latest_blockhash().await.unwrap();
        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[instruction], Some(&self.ctx.payer.pubkey()), &[&self.ctx.payer], blockhash,
        );
        self.ctx.banks_client.process_transaction(tx).await
    }

    fn collect_creator_fee(&self, creator: Pubkey, vault_creator: Pubkey) -> Instruction {
        Instruction {
            program_id: beam_program::ID,
            accounts: beam_program::accounts::CollectCreatorFee {
                creator, creator_vault: creator_vault(&vault_creator), system_program: system_program::ID,
                event_authority: event_authority(), program: beam_program::ID,
            }.to_account_metas(None),
            data: beam_program::instruction::CollectCreatorFee {}.data(),
        }
    }
}

/// The creator wallet chosen at `create_v2` (distinct from the trader who
/// creates and trades) accrues creator fees in its vault; anyone may trigger
/// `collect_creator_fee`, and the payout always lands in that wallet.
#[tokio::test]
async fn creator_fees_accrue_in_the_vault_and_pay_out_to_the_selected_wallet() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    assert_ne!(b.creator, b.user.pubkey(), "the creator wallet is not the signer");
    let (creator, vault) = (b.creator, creator_vault(&b.creator));
    let rent0 = Rent::default().minimum_balance(0);

    let curve = b.curve().await;
    let amount = 35_000_000_000_000;
    let quote = u64::try_from(buy_quote(&curve, amount).unwrap()).unwrap();
    let buy_fee = expected_fees(quote).creator;
    b.send(&[b.buy(amount, u64::MAX, false)], &[]).await.expect("buy");
    let bought = b.curve().await;
    let gross = u64::try_from(bought.sell_quote(amount / 2).unwrap()).unwrap();
    let sell_fee = expected_fees(gross).creator;
    b.send(&[b.sell(amount / 2, 0)], &[]).await.expect("sell");
    assert_eq!(b.lamports(vault).await, rent0 + buy_fee + sell_fee, "vault holds rent plus both creator fees");
    assert_eq!(b.lamports(creator).await, 0, "nothing is paid out before collection");

    // A third party (the test payer: not the creator, not the trader) triggers the payout.
    let trader_before = b.lamports(b.user.pubkey()).await;
    b.send_as_third_party(b.collect_creator_fee(creator, creator)).await.expect("collect");
    assert_eq!(b.lamports(creator).await, buy_fee + sell_fee, "the selected wallet receives every accrued fee");
    assert_eq!(b.lamports(vault).await, rent0, "the vault keeps exactly its rent-exempt minimum");
    assert_eq!(b.lamports(b.user.pubkey()).await, trader_before, "the trader receives nothing");

    // Collecting again is a no-op, and a vault derived for another key is rejected.
    b.send_as_third_party(b.collect_creator_fee(creator, creator)).await.expect("empty collect is a no-op");
    assert_eq!(b.lamports(creator).await, buy_fee + sell_fee);
    let seeds: u32 = anchor_lang::error::ErrorCode::ConstraintSeeds.into();
    assert_eq!(custom_error(b.send_as_third_party(b.collect_creator_fee(b.stranger, creator)).await), seeds,
        "a vault cannot be redirected to another wallet");
}

/// A payout that would leave a new creator wallet below rent exemption is
/// deferred, not lost: the lamports stay in the vault until enough accrues.
#[tokio::test]
async fn small_creator_fees_wait_in_the_vault_until_they_cover_rent() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    let (creator, vault) = (b.creator, creator_vault(&b.creator));
    let rent0 = Rent::default().minimum_balance(0);

    let curve = b.curve().await;
    let small = 1_000_000_000; // a tiny buy whose creator fee is far below rent exemption
    let fee = expected_fees(u64::try_from(buy_quote(&curve, small).unwrap()).unwrap()).creator;
    assert!(fee < rent0);
    b.send(&[b.buy(small, u64::MAX, false)], &[]).await.expect("small buy");
    b.send_as_third_party(b.collect_creator_fee(creator, creator)).await.expect("deferred collect succeeds");
    assert_eq!(b.lamports(creator).await, 0, "no payout below the new account's rent minimum");
    assert_eq!(b.lamports(vault).await, rent0 + fee, "the fee stays in the vault");

    let bigger = 35_000_000_000_000;
    let curve = b.curve().await;
    let fee2 = expected_fees(u64::try_from(buy_quote(&curve, bigger).unwrap()).unwrap()).creator;
    b.send(&[b.buy(bigger, u64::MAX, false)], &[]).await.expect("larger buy");
    b.send_as_third_party(b.collect_creator_fee(creator, creator)).await.expect("collect");
    assert_eq!(b.lamports(creator).await, fee + fee2, "the deferred fee is paid with the next one");
    assert_eq!(b.lamports(vault).await, rent0);
}

/// A creator account owned by the inherited fee program (a vault migrated to a
/// sharing config) cannot be paid through `collect_creator_fee`.
#[tokio::test]
async fn collection_is_refused_for_a_sharing_config_creator() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    b.send(&[b.buy(35_000_000_000_000, u64::MAX, false)], &[]).await.expect("buy");
    let creator = b.creator;
    let vault_before = b.lamports(creator_vault(&creator)).await;
    b.ctx.set_account(&creator, &solana_sdk::account::AccountSharedData::new(Rent::default().minimum_balance(8), 8, &FEE_PROGRAM));
    assert_eq!(custom_error(b.send_as_third_party(b.collect_creator_fee(creator, creator)).await),
        code(PumpError::UnableToDistributeCreatorVaultMigratedToSharingConfig));
    assert_eq!(b.lamports(creator_vault(&creator)).await, vault_before, "the vault is untouched");
}

/// The first claim would succeed by itself. A later invalid claim in the same
/// transaction must undo that transfer, including the synthetic vault debit.
#[tokio::test]
async fn late_recipient_substitution_rolls_back_an_earlier_creator_claim() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    b.send(&[b.buy(35_000_000_000_000, u64::MAX, false)], &[]).await.expect("buy");

    let (creator, stranger) = (b.creator, b.stranger);
    let vault = creator_vault(&creator);
    let before = (b.lamports(creator).await, b.lamports(stranger).await, b.lamports(vault).await);
    assert!(before.2 > Rent::default().minimum_balance(0), "a claim must be available");

    // The test payer alone signs. The second instruction's vault is derived
    // for creator, not stranger, so Anchor rejects it after the first claim.
    let instructions = [
        b.collect_creator_fee(creator, creator),
        b.collect_creator_fee(stranger, creator),
    ];
    let blockhash = b.ctx.banks_client.get_latest_blockhash().await.unwrap();
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &instructions, Some(&b.ctx.payer.pubkey()), &[&b.ctx.payer], blockhash,
    );
    let seeds: u32 = anchor_lang::error::ErrorCode::ConstraintSeeds.into();
    assert_eq!(custom_error(b.ctx.banks_client.process_transaction(tx).await), seeds);
    assert_eq!(
        (b.lamports(creator).await, b.lamports(stranger).await, b.lamports(vault).await),
        before,
        "a late failed instruction must roll back an earlier valid payout",
    );
}
