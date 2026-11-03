//! Ignored fixture producer: executes `buy_exact_quote_in_v2` on a native-SOL
//! curve for a grid of inputs and prints what the handler actually did as
//! JSON, for `sdk/fixtures/quote-exact-in.json`. Run with
//!
//!   cargo test --locked --test exact_in_fixture -- --ignored --nocapture | sed -n 's/^@@//p'
//!
//! Fixture lines carry an `@@` prefix because the native processor also
//! writes program logs to stdout.
//!
//! Each observation records the curve before the trade and the observed token,
//! curve, fee, buyback and creator-vault deltas, or the program error. The fee
//! program is the shared synthetic processor (95 protocol / 30 creator bps).
mod common;

use common::*;
use anchor_lang::{prelude::Pubkey, InstructionData, ToAccountMetas};
use anchor_spl::{
    associated_token::{get_associated_token_address_with_program_id as ata, spl_associated_token_account},
    token::spl_token::{self, native_mint::ID as WSOL},
    token_2022::spl_token_2022,
};
use beam_program::{OptionBool, PumpError};
use solana_sdk::{instruction::Instruction, rent::Rent, signature::Signer, system_program};

fn quote_ata(owner: &Pubkey) -> Pubkey { ata(owner, &WSOL, &spl_token::ID) }

fn exact_in(b: &Bank, input: u64, min_tokens_out: u64, partial: bool) -> Instruction {
    let mint = b.mint.pubkey();
    let curve = curve_pda(&mint);
    let user = b.user.pubkey();
    let vault = creator_vault(&b.creator);
    let uva = pda(&[b"user_volume_accumulator", user.as_ref()], &beam_program::ID);
    let accounts = beam_program::accounts::BuyV2 {
        global: global_pda(), base_mint: mint, quote_mint: WSOL,
        base_token_program: spl_token_2022::ID, quote_token_program: spl_token::ID,
        associated_token_program: spl_associated_token_account::ID,
        fee_recipient: b.fee_recipient, associated_quote_fee_recipient: quote_ata(&b.fee_recipient),
        buyback_fee_recipient: b.buyback, associated_quote_buyback_fee_recipient: quote_ata(&b.buyback),
        bonding_curve: curve, associated_base_bonding_curve: ata(&curve, &mint, &spl_token_2022::ID),
        associated_quote_bonding_curve: quote_ata(&curve), user,
        associated_base_user: ata(&user, &mint, &spl_token_2022::ID), associated_quote_user: quote_ata(&user),
        creator_vault: vault, associated_creator_vault: quote_ata(&vault),
        sharing_config: pda(&[b"sharing-config", mint.as_ref()], &FEE_PROGRAM),
        global_volume_accumulator: pda(&[b"global_volume_accumulator"], &beam_program::ID),
        user_volume_accumulator: uva, associated_user_volume_accumulator: quote_ata(&uva),
        fee_config: fee_config_pda(), fee_program: FEE_PROGRAM, system_program: system_program::ID,
        event_authority: event_authority(), program: beam_program::ID,
    };
    Instruction {
        program_id: beam_program::ID, accounts: accounts.to_account_metas(None),
        data: beam_program::instruction::BuyExactQuoteInV2 { spendable_quote_in: input, min_tokens_out, partial_fill: OptionBool(partial) }.data(),
    }
}

fn error_name(code: u32) -> String {
    for (name, error) in [
        ("BondingCurveComplete", PumpError::BondingCurveComplete), ("BuyZeroAmount", PumpError::BuyZeroAmount),
        ("BuyNotEnoughSolToCoverFees", PumpError::BuyNotEnoughSolToCoverFees),
        ("BuySlippageBelowMinTokensOut", PumpError::BuySlippageBelowMinTokensOut),
        ("TooMuchSolRequired", PumpError::TooMuchSolRequired), ("NotEnoughTokensToBuy", PumpError::NotEnoughTokensToBuy),
        ("Overflow", PumpError::Overflow), ("Truncation", PumpError::Truncation),
    ] {
        if code == u32::from(error) { return name.to_string(); }
    }
    format!("Custom({code})")
}

#[tokio::test]
#[ignore = "fixture producer; run explicitly with --ignored --nocapture"]
async fn print_exact_quote_in_observations() {
    let mut b = bank().await;
    let mint = b.mint.insecure_clone();
    b.send(&[b.create_v2(), b.create_user_ata()], &[&mint]).await.expect("create_v2");
    let rent0 = Rent::default().minimum_balance(0);

    // (input, min_tokens_out, partial). min_tokens_out = 1 accepts any positive
    // fill; u64::MAX forces the slippage check; 0 hits the zero-limit check.
    let mut grid: Vec<(u64, u64, bool)> = vec![
        (0, 0, false), (0, 1, false), (1, 1, false), (2, 1, false), (80, 1, false), (81, 1, false), (102, 1, false),
        (1_000_000_007, 0, false), (1_000_000_007, u64::MAX, false),
    ];
    for input in [1_000_000_007u64, 1_000_000_014, 101, 10_125, 123_456_789, 999_999_999, 5_000_000_001, 33_333_333_333] {
        grid.push((input, 1, false));
    }
    grid.push((u64::MAX / 20_000, 1, false));
    grid.push((300_000_000_000, 1, true));
    grid.push((1_000_000, 1, true));

    println!("@@{{\"provenance\":\"observed execution of buy_exact_quote_in_v2 in tests/legacy-trade-runtime (native processor, synthetic fee program)\",");
    println!("@@\"protocolFeeBps\":\"{PROTOCOL_BPS}\",\"creatorFeeBps\":\"{CREATOR_BPS}\",\"creatorFeeConfigurable\":false,\"creator\":\"{}\",", b.creator);
    println!("@@\"observations\":[");
    let mut vault_funded = false;
    for (index, (input, min_tokens_out, partial)) in grid.iter().copied().enumerate() {
        let c = b.curve().await;
        let tokens_before = b.token_balance(&b.user.pubkey()).await;
        let curve_key = curve_pda(&b.mint.pubkey());
        let vault = creator_vault(&b.creator);
        let before = [b.lamports(curve_key).await, b.lamports(b.fee_recipient).await, b.lamports(b.buyback).await, b.lamports(vault).await];
        let result = b.send(&[exact_in(&b, input, min_tokens_out, partial)], &[]).await;
        let outcome = match result {
            Ok(()) => {
                let after = [b.lamports(curve_key).await, b.lamports(b.fee_recipient).await, b.lamports(b.buyback).await, b.lamports(vault).await];
                let topup = if vault_funded { 0 } else { rent0 };
                vault_funded = true;
                format!(
                    "\"result\":{{\"tokenAmount\":\"{}\",\"quote\":\"{}\",\"protocolFee\":\"{}\",\"creatorFee\":\"{}\"}}",
                    b.token_balance(&b.user.pubkey()).await - tokens_before, after[0] - before[0],
                    (after[1] - before[1]) + (after[2] - before[2]), after[3] - before[3] - topup,
                )
            }
            error => format!("\"error\":\"{}\"", error_name(custom_error(error))),
        };
        println!(
            "@@{}{{\"curve\":{{\"virtualTokenReserves\":\"{}\",\"virtualQuoteReserves\":\"{}\",\"realTokenReserves\":\"{}\",\"realQuoteReserves\":\"{}\",\"complete\":{}}},\"spendableQuoteIn\":\"{input}\",\"minTokensOut\":\"{min_tokens_out}\",\"partialFill\":{partial},{outcome}}}",
            if index == 0 { "" } else { "," },
            c.virtual_token_reserves, c.virtual_quote_reserves, c.real_token_reserves, c.real_quote_reserves, c.complete,
        );
    }
    println!("@@]}}");
}
