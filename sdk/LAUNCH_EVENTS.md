# Local launch event routing

`decodeBeamLaunchEvent(bytes)` combines the existing raw `CreateEvent`,
`TradeEvent`, and `CompleteEvent` decoders. It returns `{ eventName, data }` for
one complete Anchor event payload, including its eight-byte discriminator.
`CompleteEvent` records **bonding-curve completion**, not migration to Raydium.
Unknown events, event-CPI envelopes, truncated data, and trailing bytes fail.
`classifyBeamLaunchPayload(bytes)` maps these local payloads to the read-only
feed vocabulary (`create`, `buy`, `sell`, `curve_complete`) and returns their
decoded mint. It never emits `migrate`. A web `IndexedEvent` still needs a
separately authenticated program, transaction signature, event position,
slot, source/finality and mint-to-curve association; this helper supplies none.

This is a local SDK interface over previously compiled Anchor fixtures:
`fixtures/events.json` and `fixtures/native-sol.json`. It adds no transaction,
RPC, signing, sending, wallet, backend, or website behavior. Decoding caller
bytes does not authenticate a program, transaction, slot, account, or migration.
The gated CPMM `BeamRaydiumMigration` event is intentionally unsupported;
there is no Beam AMM v4 migration event to decode yet.

Run `node --test sdk/beam-launch-events.test.mjs` from the repository root.
