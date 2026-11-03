//! Executes Beam's `buy_v2`, `buy_exact_quote_in_v2` and `sell_v2` handlers on
//! a native-SOL curve in a synthetic local bank. Expected values come from the
//! program's pure `buy_quote`, `sell_quote`, `tokens_for_quote` and
//! `fee_math::fee`; the exact-quote-in model restates the handler's arithmetic
//! (programs/beam/src/buy_v2.rs) for the native, non-whitelisted path.
//!
//! The inherited Pump fee program is a synthetic processor returning fixed
//! test bps. Nothing here is Beam or Pump fee policy.
mod common;

use common::*;
use anchor_lang::{prelude::Pubkey, InstructionData, ToAccountMetas};
use anchor_spl::{
    associated_token::{get_associated_token_address_with_program_id as ata, spl_associated_token_account},
    token::spl_token::{self, native_mint::ID as WSOL},
    token_2022::spl_token_2022,
};
use beam_program::{buy_quote, fee_math::fee, tokens_for_quote, trade_types::BondingCurve, OptionBool, PumpError};
use solana_sdk::{instruction::Instruction, rent::Rent, signature::Signer, system_program};

fn quote_ata(owner: &Pubkey) -> Pubkey { ata(owner, &WSOL, &spl_token::ID) }
fn u64_of(value: u128) -> u64 { u64::try_from(value).unwrap() }

#[derive(Clone, Copy)]
struct V2Accounts { fee_recipient: Pubkey, buyback: Pubkey, quote_mint: Pubkey, associated_quote_fee_recipient: Option<Pubkey> }

impl Bank {
    fn v2(&self) -> V2Accounts {
        V2Accounts { fee_recipient: self.fee_recipient, buyback: self.buyback, quote_mint: WSOL, associated_quote_fee_recipient: None }
    }

    fn v2_metas(&self, buy: bool, v: V2Accounts) -> Vec<solana_sdk::instruction::AccountMeta> {
        let mint = self.mint.pubkey();
        let curve = curve_pda(&mint);
        let user = self.user.pubkey();
        let vault = creator_vault(&self.creator);
        let uva = pda(&[b"user_volume_accumulator", user.as_ref()], &beam_program::ID);
        let fee_quote_ata = v.associated_quote_fee_recipient.unwrap_or_else(|| quote_ata(&v.fee_recipient));
        let sharing = pda(&[b"sharing-config", mint.as_ref()], &FEE_PROGRAM);
        if buy {
            beam_program::accounts::BuyV2 {
                global: global_pda(), base_mint: mint, quote_mint: v.quote_mint,
                base_token_program: spl_token_2022::ID, quote_token_program: spl_token::ID,
                associated_token_program: spl_associated_token_account::ID,
                fee_recipient: v.fee_recipient, associated_quote_fee_recipient: fee_quote_ata,
                buyback_fee_recipient: v.buyback, associated_quote_buyback_fee_recipient: quote_ata(&v.buyback),
                bonding_curve: curve, associated_base_bonding_curve: ata(&curve, &mint, &spl_token_2022::ID),
                associated_quote_bonding_curve: quote_ata(&curve), user,
                associated_base_user: ata(&user, &mint, &spl_token_2022::ID), associated_quote_user: quote_ata(&user),
                creator_vault: vault, associated_creator_vault: quote_ata(&vault), sharing_config: sharing,
                global_volume_accumulator: pda(&[b"global_volume_accumulator"], &beam_program::ID),
                user_volume_accumulator: uva, associated_user_volume_accumulator: quote_ata(&uva),
                fee_config: fee_config_pda(), fee_program: FEE_PROGRAM, system_program: system_program::ID,
                event_authority: event_authority(), program: beam_program::ID,
            }.to_account_metas(None)
        } else {
            beam_program::accounts::SellV2 {
                global: global_pda(), base_mint: mint, quote_mint: v.quote_mint,
                base_token_program: spl_token_2022::ID, quote_token_program: spl_token::ID,
                associated_token_program: spl_associated_token_account::ID,
                fee_recipient: v.fee_recipient, associated_quote_fee_recipient: fee_quote_ata,
                buyback_fee_recipient: v.buyback, associated_quote_buyback_fee_recipient: quote_ata(&v.buyback),
                bonding_curve: curve, associated_base_bonding_curve: ata(&curve, &mint, &spl_token_2022::ID),
                associated_quote_bonding_curve: quote_ata(&curve), user,
                associated_base_user: ata(&user, &mint, &spl_token_2022::ID), associated_quote_user: quote_ata(&user),
                creator_vault: vault, associated_creator_vault: quote_ata(&vault), sharing_config: sharing,
                user_volume_accumulator: uva, associated_user_volume_accumulator: quote_ata(&uva),
                fee_config: fee_config_pda(), fee_program: FEE_PROGRAM, system_program: system_program::ID,
                event_authority: event_authority(), program: beam_program::ID,
            }.to_account_metas(None)
        }
    }

    fn buy_v2(&self, amount: u64, max_sol_cost: u64, partial: bool, v: V2Accounts) -> Instruction {
        Instruction {
            program_id: beam_program::ID, accounts: self.v2_metas(true, v),
            data: beam_program::instruction::BuyV2 { amount, max_sol_cost, partial_fill: OptionBool(partial) }.data(),
        }
    }
    fn buy_exact_in_v2(&self, spendable_quote_in: u64, min_tokens_out: u64, partial: bool) -> Instruction {
        Instruction {
            program_id: beam_program::ID, accounts: self.v2_metas(true, self.v2()),
            data: beam_program::instruction::BuyExactQuoteInV2 { spendable_quote_in, min_tokens_out, partial_fill: OptionBool(partial) }.data(),
        }
    }
    fn sell_v2(&self, amount: u64, min_sol_output: u64) -> Instruction {
        Instruction {
            program_id: beam_program::ID, accounts: self.v2_metas(false, self.v2()),
            data: beam_program::instruction::SellV2 { amount, min_sol_output }.data(),
        }
    }

    async fn balances(&mut self) -> [u64; 4] {
        let curve = curve_pda(&self.mint.pubkey());
        let vault = creator_vault(&self.creator);
        [self.lamports(curve).await, self.lamports(self.fee_recipient).await, self.lamports(self.buyback).await, self.lamports(vault).await]
    }

    async fn created(&mut self) -> BondingCurve {
        let mint = self.mint.insecure_clone();
        self.send(&[self.create_v2(), self.create_user_ata()], &[&mint]).await.expect("create_v2");
        self.curve().await
    }
}

fn buyback_share(protocol: u64) -> u64 { u64_of(u128::from(BUYBACK_BPS) * u128::from(protocol) / 10_000) }

/// Native, non-whitelisted `buy_exact_quote_in_handler`: fee bps are applied
/// to the largest quote whose fees fit, then the quote is trimmed if rounding
/// pushes the total over `input`. Returns (tokens, quote, protocol, creator).
fn exact_quote_in(curve: &BondingCurve, input: u64, partial: bool) -> (u64, u64, u64, u64) {
    let total_bps = PROTOCOL_BPS + CREATOR_BPS;
    let raw = u64_of(u128::from(input) * 10_000 / (10_000 + u128::from(total_bps)));
    let (pf, cf) = (fee(raw, PROTOCOL_BPS).unwrap(), fee(raw, CREATOR_BPS).unwrap());
    let total = raw + pf + cf;
    let quote = if total > input { raw - (total - input) } else { raw };
    let computed = u64_of(tokens_for_quote(curve, quote).unwrap());
    if computed > curve.real_token_reserves && partial {
        let amount = curve.real_token_reserves;
        let quote = u64_of(buy_quote(curve, amount).unwrap());
        let (pf, cf) = (fee(quote, PROTOCOL_BPS).unwrap(), fee(quote, CREATOR_BPS).unwrap());
        assert!(input >= quote + pf + cf);
        return (amount, quote, pf, cf);
    }
    if quote + pf + cf > input {
        let quote = quote - (quote + pf + cf - input);
        return (u64_of(tokens_for_quote(curve, quote).unwrap()), quote, pf, cf);
    }
    (computed, quote, pf, cf)
}

#[tokio::test]
async fn buy_v2_and_sell_v2_match_pure_math_at_exact_bounds() {
    let mut b = bank().await;
    let curve = b.created().await;
    let v = b.v2();

    let amount = 35_000_000_000_000;
    let quote = u64_of(buy_quote(&curve, amount).unwrap());
    let (pf, cf) = (fee(quote, PROTOCOL_BPS).unwrap(), fee(quote, CREATOR_BPS).unwrap());
    let total = quote + pf + cf;
    assert_eq!(custom_error(b.send(&[b.buy_v2(amount, total - 1, false, v)], &[]).await), code(PumpError::TooMuchSolRequired));
    let before = b.balances().await;
    b.send(&[b.buy_v2(amount, total, false, v)], &[]).await.expect("buy_v2 at exact bound");
    let after = b.balances().await;
    assert_eq!(after[0] - before[0], quote, "curve receives the quote");
    assert_eq!(after[1] - before[1], pf - buyback_share(pf));
    assert_eq!(after[2] - before[2], buyback_share(pf));
    assert_eq!(after[3] - before[3], Rent::default().minimum_balance(0) + cf);
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount);
    let bought = b.curve().await;
    assert_eq!((bought.virtual_token_reserves, bought.virtual_quote_reserves), (curve.virtual_token_reserves - amount, curve.virtual_quote_reserves + quote));

    let sold = amount / 2;
    let gross = u64_of(bought.sell_quote(sold).unwrap());
    let (pf, cf) = (fee(gross, PROTOCOL_BPS).unwrap(), fee(gross, CREATOR_BPS).unwrap());
    let net = gross - pf - cf;
    assert_eq!(custom_error(b.send(&[b.sell_v2(sold, net + 1)], &[]).await), code(PumpError::TooLittleSolReceived));
    let user_before = b.lamports(b.user.pubkey()).await;
    let before = b.balances().await;
    b.send(&[b.sell_v2(sold, net)], &[]).await.expect("sell_v2 at exact bound");
    let after = b.balances().await;
    assert_eq!(b.lamports(b.user.pubkey()).await - user_before, net);
    assert_eq!(before[0] - after[0], gross, "curve pays the gross quote");
    assert_eq!(after[1] - before[1], pf - buyback_share(pf));
    assert_eq!(after[2] - before[2], buyback_share(pf));
    assert_eq!(after[3] - before[3], cf);
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount - sold);
}

#[tokio::test]
async fn buy_exact_quote_in_v2_spends_at_most_the_input_and_honours_min_tokens() {
    let mut b = bank().await;
    let curve = b.created().await;
    let input = 1_000_000_007;
    let (amount, quote, pf, cf) = exact_quote_in(&curve, input, false);
    assert!(quote + pf + cf <= input, "never spends more than the input");

    assert_eq!(custom_error(b.send(&[b.buy_exact_in_v2(input, amount + 1, false)], &[]).await), code(PumpError::BuySlippageBelowMinTokensOut));
    let user_before = b.lamports(b.user.pubkey()).await;
    let before = b.balances().await;
    b.send(&[b.buy_exact_in_v2(input, amount, false)], &[]).await.expect("exact-in buy at min tokens");
    let after = b.balances().await;
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount);
    assert_eq!(after[0] - before[0], quote);
    assert_eq!(after[1] - before[1], pf - buyback_share(pf));
    assert_eq!(after[2] - before[2], buyback_share(pf));
    assert_eq!(after[3] - before[3], Rent::default().minimum_balance(0) + cf);
    let uva_rent = Rent::default().minimum_balance(137);
    assert_eq!(user_before - b.lamports(b.user.pubkey()).await, quote + pf + cf + uva_rent + Rent::default().minimum_balance(0),
        "user pays quote, fees, the volume-accumulator rent and the creator-vault top-up");
    let bought = b.curve().await;
    assert_eq!(bought.virtual_quote_reserves, curve.virtual_quote_reserves + quote);
    assert_eq!(bought.virtual_token_reserves, curve.virtual_token_reserves - amount);

    // 1_000_000_014 lamports is the first input above 1e9 where rounding the
    // two ceiling fees pushes raw + fees over the input, so the handler trims
    // the quote. The fees stay computed on the untrimmed quote.
    let input = 1_000_000_014;
    let raw = u64_of(u128::from(input) * 10_000 / (10_000 + u128::from(PROTOCOL_BPS + CREATOR_BPS)));
    let (amount2, quote, pf, cf) = exact_quote_in(&bought, input, false);
    assert_eq!(quote, raw - 1, "trim branch is exercised");
    assert_eq!((pf, cf), (fee(raw, PROTOCOL_BPS).unwrap(), fee(raw, CREATOR_BPS).unwrap()));
    assert_eq!(quote + pf + cf, input, "after the trim the user spends exactly the input");
    let user_before = b.lamports(b.user.pubkey()).await;
    let before = b.balances().await;
    b.send(&[b.buy_exact_in_v2(input, amount2, false)], &[]).await.expect("trimmed exact-in buy");
    let after = b.balances().await;
    assert_eq!(user_before - b.lamports(b.user.pubkey()).await, input);
    assert_eq!(after[0] - before[0], quote);
    assert_eq!(after[1] - before[1], pf - buyback_share(pf));
    assert_eq!(after[3] - before[3], cf);
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount + amount2);

    assert_eq!(custom_error(b.send(&[b.buy_exact_in_v2(input, 0, false)], &[]).await), code(PumpError::BuyZeroAmount));
    assert_eq!(custom_error(b.send(&[b.buy_exact_in_v2(0, 1, false)], &[]).await), code(PumpError::BuyNotEnoughSolToCoverFees));
}

#[tokio::test]
async fn exact_quote_in_partial_fill_completes_the_curve() {
    let mut b = bank().await;
    let curve = b.created().await;
    let input = 200_000_000_000;
    let (amount, quote, pf, cf) = exact_quote_in(&curve, input, true);
    assert_eq!(amount, curve.real_token_reserves);
    let user_before = b.lamports(b.user.pubkey()).await;
    b.send(&[b.buy_exact_in_v2(input, 1, true)], &[]).await.expect("partial exact-in buy");
    let done = b.curve().await;
    assert!(done.complete);
    assert_eq!(done.real_token_reserves, 0);
    assert_eq!(b.token_balance(&b.user.pubkey()).await, amount);
    let spent = user_before - b.lamports(b.user.pubkey()).await;
    assert_eq!(spent, quote + pf + cf + Rent::default().minimum_balance(137) + Rent::default().minimum_balance(0));
    assert!(spent < input, "the clamped buy charges only the quote for the remaining reserves");
    assert_eq!(custom_error(b.send(&[b.sell_v2(1, 0)], &[]).await), code(PumpError::BondingCurveComplete));
}

#[tokio::test]
async fn v2_rejects_wrong_quote_mint_recipients_and_quote_accounts() {
    let mut b = bank().await;
    b.created().await;
    let base = b.v2();
    let stranger = b.stranger;

    let wrong_mint = V2Accounts { quote_mint: b.mint.pubkey(), ..base };
    let mismatch: u32 = PumpError::MintDoesNotMatchBondingCurve.into();
    let result = custom_error(b.send(&[b.buy_v2(1_000_000, u64::MAX, false, wrong_mint)], &[]).await);
    let seeds: u32 = anchor_lang::error::ErrorCode::ConstraintSeeds.into();
    // The quote ATAs are derived from quote_mint, so either the curve constraint
    // or an ATA seed constraint rejects the substitution first.
    assert!(result == mismatch || result == seeds, "wrong quote mint rejected, got {result}");

    let wrong_buyback = V2Accounts { buyback: stranger, ..base };
    assert_eq!(custom_error(b.send(&[b.buy_v2(1_000_000, u64::MAX, false, wrong_buyback)], &[]).await), code(PumpError::BuybackFeeRecipientNotAuthorized));
    let wrong_recipient = V2Accounts { fee_recipient: stranger, ..base };
    assert_eq!(custom_error(b.send(&[b.buy_v2(1_000_000, u64::MAX, false, wrong_recipient)], &[]).await), code(PumpError::NotAuthorized));
    let wrong_quote_ata = V2Accounts { associated_quote_fee_recipient: Some(ata(&b.fee_recipient, &WSOL, &spl_token_2022::ID)), ..base };
    assert_eq!(custom_error(b.send(&[b.buy_v2(1_000_000, u64::MAX, false, wrong_quote_ata)], &[]).await), seeds);
    assert_eq!(custom_error(b.send(&[b.buy_v2(0, u64::MAX, false, base)], &[]).await), code(PumpError::BuyZeroAmount));
}
