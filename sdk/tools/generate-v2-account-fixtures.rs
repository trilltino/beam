//! Host-only BuyV2/SellV2 account-meta fixture producer for a native-SOL curve.
//! No handler, wallet, RPC or transaction. Run as an example of the existing,
//! locked beam-program package (copy to
//! programs/beam/examples/beam_v2_account_fixture.rs, run, then delete):
//!
//!   cargo run --locked --manifest-path programs/beam/Cargo.toml --example beam_v2_account_fixture
//!
//! Ordered metas come from Anchor's generated `accounts::BuyV2` /
//! `accounts::SellV2` `to_account_metas`, filled with addresses derived from
//! the seeds in programs/beam/src/buy_v2.rs and sell_v2.rs. Inputs are
//! synthetic. The same derivations are accepted by the real handlers in
//! tests/legacy-trade-runtime/tests/v2_trade.rs.
use anchor_lang::{prelude::Pubkey, solana_program::hash::hashv, ToAccountMetas};
use anchor_spl::associated_token::get_associated_token_address_with_program_id as ata;

fn synthetic(label: &str) -> Pubkey { Pubkey::new_from_array(hashv(&[b"beam-v2-fixture", label.as_bytes()]).to_bytes()) }
fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey { Pubkey::find_program_address(seeds, program).0 }

fn main() {
    let beam = beam_program::ID;
    let fees = beam_program::pump_fees::ID;
    let wsol = anchor_spl::token::spl_token::native_mint::ID;
    let token = anchor_spl::token::ID;
    let (mint, user, creator) = (synthetic("mint"), synthetic("user"), synthetic("creator"));
    let (fee_recipient, buyback) = (synthetic("fee-recipient"), synthetic("buyback"));
    let curve = pda(&[b"bonding-curve", mint.as_ref()], &beam);
    let vault = pda(&[b"creator-vault", creator.as_ref()], &beam);
    let uva = pda(&[b"user_volume_accumulator", user.as_ref()], &beam);
    let fee_config = pda(&[b"fee_config", beam.as_ref()], &fees);
    let quote = |owner: &Pubkey| ata(owner, &wsol, &token);

    println!("{{\"provenance\":\"Anchor accounts::BuyV2/SellV2 to_account_metas via the locked beam-program crate; synthetic inputs only\",");
    println!("\"inputs\":{{\"mint\":\"{mint}\",\"user\":\"{user}\",\"creator\":\"{creator}\",\"feeRecipient\":\"{fee_recipient}\",\"buybackFeeRecipient\":\"{buyback}\",\"feeConfig\":\"{fee_config}\",\"feeProgram\":\"{fees}\"}},");
    println!("\"cases\":[");
    let mut first = true;
    for (base_program, label) in [(anchor_spl::token_2022::ID, "token2022"), (token, "token")] {
        let base = |owner: &Pubkey| ata(owner, &mint, &base_program);
        let buy = beam_program::accounts::BuyV2 {
            global: pda(&[b"global"], &beam), base_mint: mint, quote_mint: wsol,
            base_token_program: base_program, quote_token_program: token,
            associated_token_program: anchor_spl::associated_token::ID,
            fee_recipient, associated_quote_fee_recipient: quote(&fee_recipient),
            buyback_fee_recipient: buyback, associated_quote_buyback_fee_recipient: quote(&buyback),
            bonding_curve: curve, associated_base_bonding_curve: base(&curve), associated_quote_bonding_curve: quote(&curve),
            user, associated_base_user: base(&user), associated_quote_user: quote(&user),
            creator_vault: vault, associated_creator_vault: quote(&vault),
            sharing_config: pda(&[b"sharing-config", mint.as_ref()], &fees),
            global_volume_accumulator: pda(&[b"global_volume_accumulator"], &beam),
            user_volume_accumulator: uva, associated_user_volume_accumulator: quote(&uva),
            fee_config, fee_program: fees, system_program: anchor_lang::solana_program::system_program::ID,
            event_authority: pda(&[b"__event_authority"], &beam), program: beam,
        }.to_account_metas(None);
        let sell = beam_program::accounts::SellV2 {
            global: pda(&[b"global"], &beam), base_mint: mint, quote_mint: wsol,
            base_token_program: base_program, quote_token_program: token,
            associated_token_program: anchor_spl::associated_token::ID,
            fee_recipient, associated_quote_fee_recipient: quote(&fee_recipient),
            buyback_fee_recipient: buyback, associated_quote_buyback_fee_recipient: quote(&buyback),
            bonding_curve: curve, associated_base_bonding_curve: base(&curve), associated_quote_bonding_curve: quote(&curve),
            user, associated_base_user: base(&user), associated_quote_user: quote(&user),
            creator_vault: vault, associated_creator_vault: quote(&vault),
            sharing_config: pda(&[b"sharing-config", mint.as_ref()], &fees),
            user_volume_accumulator: uva, associated_user_volume_accumulator: quote(&uva),
            fee_config, fee_program: fees, system_program: anchor_lang::solana_program::system_program::ID,
            event_authority: pda(&[b"__event_authority"], &beam), program: beam,
        }.to_account_metas(None);
        for (kind, metas) in [("buyV2", buy), ("sellV2", sell)] {
            let list: Vec<String> = metas.iter().map(|m| format!(
                "{{\"pubkey\":\"{}\",\"isSigner\":{},\"isWritable\":{}}}", m.pubkey, m.is_signer, m.is_writable
            )).collect();
            println!(
                "{}{{\"instruction\":\"{kind}\",\"baseTokenProgram\":\"{base_program}\",\"label\":\"{label}\",\"metas\":[{}]}}",
                if first { "" } else { "," }, list.join(",")
            );
            first = false;
        }
    }
    println!("]}}");
}
