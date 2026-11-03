//! Byte offsets that Beam-side readers hard-code for the pinned AMM v4 `AmmInfo`.
//!
//! Draft PR #49's `beam_amm_v4_quote.rs` and `validate_pool_identity` read the pool
//! account by raw offset (status, pending PnL, vault keys, mints, LP mint, market seed,
//! target orders). A drifted offset would silently misprice a quote, so this pins each
//! one to the struct layout in raydium-io/raydium-amm@91739d8 (the crate version this
//! validator builds against). It runs without an SBF build.
use core::mem::{offset_of, size_of};
use raydium_amm::state::AmmInfo;

#[test]
fn pool_offsets_used_by_beam_readers_match_the_pinned_layout() {
    assert_eq!(size_of::<AmmInfo>(), 752, "packed AmmInfo size");
    assert_eq!(offset_of!(AmmInfo, status), 0);
    assert_eq!(offset_of!(AmmInfo, nonce), 8);
    // Pending PnL that the quote must subtract from vault balances.
    assert_eq!(offset_of!(AmmInfo, state_data.need_take_pnl_coin), 192);
    assert_eq!(offset_of!(AmmInfo, state_data.need_take_pnl_pc), 200);
    // Identity fields compared against addresses Beam derives itself.
    assert_eq!(offset_of!(AmmInfo, coin_vault), 336);
    assert_eq!(offset_of!(AmmInfo, pc_vault), 368);
    assert_eq!(offset_of!(AmmInfo, coin_vault_mint), 400);
    assert_eq!(offset_of!(AmmInfo, pc_vault_mint), 432);
    assert_eq!(offset_of!(AmmInfo, lp_mint), 464);
    assert_eq!(offset_of!(AmmInfo, market), 528);
    assert_eq!(offset_of!(AmmInfo, target_orders), 592);
}

/// The extra offsets read by `beam_amm_v4_pool_state.rs`, which compares a fresh pool field by field.
#[test]
fn pool_state_offsets_match_the_pinned_layout() {
    assert_eq!(offset_of!(AmmInfo, coin_decimals), 32);
    assert_eq!(offset_of!(AmmInfo, pc_decimals), 40);
    assert_eq!(offset_of!(AmmInfo, fees), 128);
    assert_eq!(size_of::<raydium_amm::state::Fees>(), 64, "eight u64 fee words");
    assert_eq!(offset_of!(AmmInfo, state_data.pool_open_time), 224);
    assert_eq!(offset_of!(AmmInfo, open_orders), 496);
    assert_eq!(offset_of!(AmmInfo, market_program), 560);
    assert_eq!(offset_of!(AmmInfo, lp_amount), 720);
}

/// `PINNED_FEES` in the program must equal what Raydium's own initializer writes into a new pool.
#[test]
fn pinned_fee_words_equal_raydiums_initializer() {
    let mut fees = raydium_amm::state::Fees::default();
    fees.initialize().expect("fees initialize");
    let words = [
        fees.min_separate_numerator,
        fees.min_separate_denominator,
        fees.trade_fee_numerator,
        fees.trade_fee_denominator,
        fees.pnl_numerator,
        fees.pnl_denominator,
        fees.swap_fee_numerator,
        fees.swap_fee_denominator,
    ];
    assert_eq!(words, [5, 10_000, 25, 10_000, 12, 100, 25, 10_000]);
}

/// `TARGET_ORDERS_LEN` in the program sizes the rent Beam budgets for the account Raydium creates.
#[test]
fn target_orders_size_equals_the_constant_beam_budgets_rent_for() {
    assert_eq!(size_of::<raydium_amm::state::TargetOrders>(), 2208, "packed TargetOrders size");
}
