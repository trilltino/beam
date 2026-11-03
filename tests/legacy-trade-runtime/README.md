# Native-SOL trade local bank

This crate runs Beam's native-SOL `create_v2`, legacy `buy` and `sell`, and V2
`buy_v2`, `buy_exact_quote_in_v2` and `sell_v2` handlers inside a
`solana-program-test` 2.2.1 bank. Shared setup lives in `tests/common/mod.rs`. It loads Beam as a native
processor, in the same way as the draft `tests/runtime-harness`.

```sh
cd tests/legacy-trade-runtime
cargo test --locked
cargo test --locked --features experimental-raydium
```

It runs no SBF build, live network, wallet or deployed program. It is not a
Beam fee path. The lockfile starts from the draft runtime harness's resolved
versions. Only this package's name and its direct `anchor-spl` dependency
differ.

## Synthetic setup

- **`Global`.** Deserialized from zeroed bytes, then given the Pump launch
  reserves (1.073e15 virtual tokens, 30 SOL virtual, 7.931e14 real tokens,
  1e15 supply). It also sets `create_v2_enabled`, one fee recipient, eight
  copies of one buyback recipient, and 2,000 buyback bps.
- **Fee program.** The inherited Pump fee program (`pfeeUx…`) is replaced by
  a synthetic processor. That processor checks the `get_fees_with_quote_mint`
  arguments and returns 95 protocol / 30 creator bps. These numbers are test
  inputs, not Beam or Pump policy. The fee CPI source and Beam's fee custody
  remain open owner decisions.
- **`fee_config` and `global_volume_accumulator`.** Both are injected at their
  PDAs, with Anchor discriminators.
- **WSOL mint.** An initialised WSOL mint is injected, because V2 handlers take
  it as `quote_mint` for native-SOL curves. They require the quote ATAs as
  addresses, but pay in lamports.

## What the tests show

1. **Create, then trade at exact bounds.** `create_v2` initialises the curve
   from `Global` with Token-2022, mints the full supply to the curve ATA and
   revokes the mint authority.
   - `buy` succeeds when `max_sol_cost` equals `buy_quote` plus fees. It fails
     with `TooMuchSolRequired` one lamport below that.
   - `sell` succeeds when `min_sol_output` equals `sell_quote` minus fees. It
     fails with `TooLittleSolReceived` one lamport above that.
   - Curve reserves change by exactly the quote. The user receives the net
     amount. The fee recipient, buyback recipient and creator vault receive
     the protocol remainder, the floor of the buyback share, and the creator
     fee. On the first buy, the creator vault is also topped up to rent
     exemption.
   - Expected values come from the program's own pure `buy_quote`,
     `sell_quote` and `fee_math::fee`.
2. **Partial fill and completion.** With partial fill, a buy is clamped to
   `real_token_reserves` and completes the curve. Without partial fill, it
   fails with `NotEnoughTokensToBuy`. After completion, both buy and sell
   fail with `BondingCurveComplete`.
3. **Rejected inputs.** The 16 base buy metas without remaining accounts are
   rejected with `BuybackFeeRecipientMissing`. A substituted fee recipient
   fails with `NotAuthorized`, and a creator vault derived from another key
   fails with `ConstraintSeeds`. Zero-amount buys and sells are rejected.

4. **V2 at exact bounds (`tests/v2_trade.rs`).**
   - `buy_v2` and `sell_v2` succeed at the exact `max_sol_cost` /
     `min_sol_output` and fail one lamport past it, as the legacy handlers do.
     The curve, fee recipient, buyback recipient and creator vault deltas
     match.
   - The buyback recipient is a named account, not a remaining account.
5. **`buy_exact_quote_in_v2`.** The test restates the handler's native
   arithmetic and checks it against the runtime:
   - The quote is grossed down: `raw = input · 10 000 / (10 000 + protocol
     bps + creator bps)`.
   - Both ceiling fees are charged on `raw`.
   - The quote is trimmed when rounding pushes the total past `input`. The
     first such input above 1 SOL is 1 000 000 014 lamports. There the user
     spends exactly `input`, while the fees stay computed on the untrimmed
     quote.
   - Tokens equal `tokens_for_quote(quote)`. One token above that as
     `min_tokens_out` fails with `BuySlippageBelowMinTokensOut`.
   - The handler has a second trim branch after the first. With fees on
     `raw`, that branch cannot change the result, because the first trim
     already makes `quote + fees == input`.
   - With partial fill, the buy clamps to the real reserves, charges only
     `buy_quote` for them and completes the curve.
6. **V2 rejected inputs.** A non-WSOL quote mint, an unlisted buyback or fee
   recipient, a quote ATA derived for the wrong token program, and a zero
   amount are each rejected.

7. **Exact-quote-in fixture producer (`tests/exact_in_fixture.rs`).** This
   test is ignored by default. It runs `buy_exact_quote_in_v2` over a grid of
   inputs on one curve and prints the observed token, quote, fee and
   creator-fee deltas, or the program error, as JSON. Its output backs the
   SDK's exact-quote-in preview:

   ```sh
   cargo test --locked --test exact_in_fixture -- --ignored --nocapture | sed -n 's/^@@//p'
   ```

8. **Creator fee collection (`tests/legacy_trade.rs`).**
   - `create_v2` takes the creator wallet as an argument. In the tests it is
     a wallet distinct from the signing trader.
   - Legacy buys and sells accrue the creator fee in the
     `["creator-vault", creator]` PDA.
   - `collect_creator_fee` needs no signer. In the test it is triggered by a
     third party, the test payer, and it pays everything above the vault's
     rent-exempt minimum to that creator wallet, never to the caller. A
     second call is a no-op. A vault derived for another wallet fails with
     `ConstraintSeeds`.
   - **Small fees wait in the vault.** A fee too small to make a new creator
     account rent-exempt is not paid out and is not lost: it stays in the
     vault and is paid with the next larger accrual.
   - A creator account owned by the inherited fee program (a vault migrated
     to a sharing config) is refused with
     `UnableToDistributeCreatorVaultMigratedToSharingConfig`, and the vault
     is left untouched.

## Remaining accounts

Legacy `buy` and `sell` read `remaining_accounts[0]` as the
`["bonding-curve-v2", mint]` PDA. They check its key only when a creator fee
is charged. They read `remaining_accounts[1]` as a buyback recipient listed
in `Global.buyback_fee_recipients`. The SDK `buildBuy` / `buildSell` helpers
return only the base metas, so a client must append both accounts.

## Not covered

- Real fee-program behaviour, fee tiers by market cap, or Beam's planned
  0.50% / 42-5-3 policy.
- Whitelist-PDA trades, cashback, holder rewards, Mayhem, non-SOL quote
  mints, Token-2022 transfer fees, V3 handlers, sharing configs and their
  `distribute_creator_fees` paths, `collect_creator_fee_v2`, `set_creator`,
  and volume tracking.
- Migration after completion, an SBF binary, compute-unit limits or a
  deployed program.
