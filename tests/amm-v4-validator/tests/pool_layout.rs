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
