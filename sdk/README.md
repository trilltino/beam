# Beam native-SOL ABI fixtures

`beam-native.mjs` is a dependency-free, local-only encoder for the retained
legacy `create`, `buy`, and `sell` instruction data and ordered Anchor account
metas at Beam contract commit `65c96b5`. It also covers `create_v2` at
`c8dc348` and decodes the raw `CompleteEvent` payload and current 166-byte
`BondingCurve` account. Run:

```sh
node --test sdk/beam-native.test.mjs
```

The golden instruction bytes in `fixtures/native-sol.json` came from
`beam_program::instruction::{Create,Buy,Sell}.data()`. The signer/writable
flags and order came from
`beam_program::accounts::{Create,LegacyBuy,LegacySell}.to_account_metas(None)`
under Anchor 0.31.1. The event bytes came from `CompleteEvent::data()` and the
curve bytes from `BondingCurve::try_serialize`. The fixture uses synthetic
public keys and `.invalid` metadata URL; it is never a transaction example.

`fixtures/create-v2.json` was generated with a temporary Rust harness linked
to the compiled `beam-program` at `c8dc348`. Its instruction bytes came from
`beam_program::instruction::CreateV2.data()` and its 16 ordered metas from
`beam_program::accounts::CreateV2.to_account_metas(None)` under Anchor 0.31.1.
The fixture uses synthetic keys, including inert Mayhem account slots and an
`.invalid` metadata URI. `encodeCreateV2` takes `creatorFeeBps` as a u64
bigint, safe integer, or decimal string. It enforces the contract's 32/13/200
UTF-8 byte limits and rejects Mayhem and deprecated cashback creation. Holder
rewards remain an allowed flag, but the program replaces the supplied creator
with a holder-rewards PDA when that flag is enabled and requires global holder
rewards to be enabled. For native SOL creation, the handler uses its fee
schedule and does not apply a caller-supplied configurable creator fee rate.

These helpers **do not** derive or verify PDAs, associated token accounts,
fee recipients, token program ownership, remaining accounts, signatures,
global feature settings, or on-chain state. `CreateV2` uses Token-2022 and its
normal-mode ABI still requires the retained Mayhem account slots; the builder
does not populate or verify their canonical addresses. In particular,
inherited Pump `fee_config` and `fee_program` accounts remain unresolved in
both legacy buy/sell and `buy_v2`/`sell_v2`; those trade handlers expect
additional validated remaining accounts. A CreateV2 fixture does not make
subsequent trading executable. The builders return base
account metadata for inspection and testing, not an executable Beam client.
The development program ID is not deployed. No RPC, wallet, signing, or live
trade code is present.

`decodeCurrentBondingCurve` intentionally rejects historical shorter curve
layouts that the Rust program can read; it accepts only exactly 166 bytes of
the current serialized layout. Neither decoder verifies the account owner or
that a payload came from a trusted transaction. Check those separately when a
real client is designed.

## Address derivation (`beam-pdas.mjs`)

`beam-pdas.mjs` is a dependency-free, synchronous derivation module for the
account slots that the builders above leave to the caller. Run:

```sh
node --test sdk/beam-pdas.test.mjs
```

It implements Solana's `create_program_address` / `find_program_address`
(bumps 255 down to 1, as Solana does) with its own SHA-256 and the
curve25519-dalek point-decompression rule. On top of that it derives the
Beam PDAs named in the Anchor constraints of `Create`, `CreateV2`,
`LegacyBuy` and `LegacySell`: `global`, `mint-authority`,
`__event_authority`, `global_volume_accumulator`, `bonding-curve`,
`creator-vault` and `user_volume_accumulator`. It also derives the Metaplex
metadata PDA, the three retained Mayhem slots that `CreateV2` checks with
`seeds::program = MAYHEM_ID`, and associated token addresses for SPL Token
and Token-2022. The `resolve*Accounts` helpers return an `accounts` object
that the matching `build*` function accepts.

`fixtures/pdas.json` is the unedited stdout of `tools/generate-pda-fixtures.rs`,
linked to the locked `beam-program` crate at `c707f30`. It uses
`Pubkey::find_program_address`, spl-associated-token-account and
`Pubkey::is_on_curve` with synthetic inputs and the program IDs compiled
into the crate. `fixtures/pdas.receipt.json` records the command, toolchain
and the SHA-256 of the source and output. The tests check those hashes, every
PDA and ATA, and 106 curve-membership vectors, including the sign bit,
non-canonical `y` and the identity point.

Boundaries:

- Derivation reads no chain state. It does not show that an account exists,
  is owned by the expected program or holds valid data.
- The inherited Pump `feeRecipient`, `feeConfig` and `feeProgram` accounts
  remain unresolved for Beam. The trade resolvers require them explicitly
  and never default them.
- The trade resolvers also require `creator`, which must be the decoded
  bonding curve's `creator` field, and `tokenProgram`, which must be the
  mint's owner. They cannot verify either value.
- `associatedUser` defaults to the user's associated token account. The
  program accepts any token account for that mint, and this module does not
  create one.
- `CreateV2`'s `mayhem_token_vault` is unconstrained in Beam, so the caller
  must supply it.
- No RPC, wallet, signing or transaction submission is present. The program
  ID is still the undeployed development placeholder.

### Legacy trade remaining accounts and `Global`

Legacy `buy` and `sell` need two accounts after the base metas: the
`["bonding-curve-v2", mint]` PDA and a buyback recipient listed in
`Global.buyback_fee_recipients`, which is writable. `buildBuy` / `buildSell`
return only the base metas. `legacyTradeRemainingAccounts` builds the pair,
and `withRemainingAccounts` appends it to a built instruction without
mutating it. Without both accounts, the handler fails with
`BuybackFeeRecipientMissing`. `tests/legacy-trade-runtime` shows this by
running the real handler in a local bank. That test is in a separate draft
PR.

`beam-global.mjs` decodes the `Global` account: 8 discriminator bytes and
1,080 bytes of fields, with trailing bytes ignored as Anchor ignores them. It
also mirrors the two recipient checks:

- `isAuthorizedFeeRecipient` mirrors `validate_fee_recipient`. The program
  looks up `fee_recipients` with Rust's `binary_search`. On an unsorted list
  that result cannot be predicted, so a listed key in an unsorted list throws
  instead of guessing.
- `isAuthorizedBuybackFeeRecipient` mirrors
  `Global::validate_buyback_fee_recipient`.

Which listed buyback recipient a client uses is left to the caller.

`fixtures/global.json` is the unedited stdout of
`tools/generate-global-fixtures.rs`. It contains Anchor `AccountSerialize`
bytes for four synthetic cases: zero, distinct values, an unsorted
fee-recipient list, and trailing padding. The fixture's receipt records its
hashes. The decoder does not verify the account owner, address or freshness.
Cashback-curve sells use a different remaining-account layout and are not
modelled; Beam rejects cashback creation.

### V2 trade accounts (native-SOL curves)

`resolveBuyV2Accounts` and `resolveSellV2Accounts` return the 27 BuyV2 and 26
SellV2 accounts. The keys are the Anchor field names that the draft
`beam-v2-trades.mjs` builders take. On a native-SOL curve the handlers require
`quoteMint` = WSOL and every quote ATA derived with SPL Token, while paying in
lamports.

The resolvers derive:

- the curve, the creator vault and both volume accumulators;
- the base and quote ATAs;
- the event authority;
- `sharing_config`. The program derives it under the hard-coded inherited
  Pump fee program ID (`INHERITED_FEE_PROGRAM_ID`), not under the
  `fee_program` account, so the resolvers do the same.

These stay explicit inputs, as in the legacy resolvers:

- `creator`
- `baseTokenProgram` (the mint's owner)
- the fee recipient and the buyback recipient. Membership of the buyback
  recipient is checked when a decoded `Global` is passed.
- `feeConfig` and `feeProgram`

`fixtures/v2-accounts.json` is the unedited stdout of
`tools/generate-v2-account-fixtures.rs`. It contains Anchor's own
`accounts::BuyV2` / `accounts::SellV2` `to_account_metas` for Token-2022 and
SPL Token base mints. The resolvers, laid out in that field order, reproduce
every pubkey and privilege flag. The same derivations run against the real
handlers in `tests/legacy-trade-runtime/tests/v2_trade.rs`, which is in a
separate draft PR.
