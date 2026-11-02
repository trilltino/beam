# Binding a legacy creator-fee recipient

`buildCollectCreatorFeeForExpectedRecipient` wraps the local-only PR #13 legacy
builder and uses the dependency-free PDA derivation from PR #45. A caller
supplies an **independently verified** `{ creator, creatorVault }` pair; the
helper rejects a claim instruction whose creator or vault differs from that
pair, a noncanonical creator-vault PDA, or a substituted event-authority PDA.
It keeps the five-account, no-creator-signature ABI unchanged. It does not
add a payout-wallet field or alter creator fees.

`previewCollectCreatorFeeForExpectedRecipient` applies the same canonical
expected pair to PR #13's caller-supplied local rent/balance preview. It
rejects a substituted creator or vault and retains the preview's own slot-age
and numeric checks. A matching preview still does not authenticate balances,
the curve, or any actual fee entitlement.

The caller must authenticate the curve's creator from trusted chain data.
The helper derives the vault from `[b"creator-vault", creator]` and checks the
event-authority PDA for the configured Beam program. It does not verify the
owner, data or finality of an on-chain account, check a claim amount, sign,
submit, or prove that a transfer will succeed. A caller that supplies a
forged creator with its own canonical vault can still produce a misleading
local preview; on-chain account constraints remain the execution boundary.

From the repository root, run:

```sh
node --test sdk/beam-creator-recipient.test.mjs
```
