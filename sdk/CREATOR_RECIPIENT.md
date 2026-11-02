# Binding a legacy creator-fee recipient

`buildCollectCreatorFeeForExpectedRecipient` wraps the local-only PR #13 legacy
builder. A caller supplies an **independently verified** `{ creator,
creatorVault }` pair; the helper rejects a claim instruction whose creator or
vault differs from that pair. It keeps the five-account, no-creator-signature
ABI unchanged. It does not add a payout-wallet field or alter creator fees.

The caller must authenticate the curve's creator and derive/check the vault
PDA from `[b"creator-vault", creator]` for the configured Beam program before
passing the expected pair. This helper checks consistency only. It does not
derive the PDA, verify the owner or finality of chain data, check a claim
amount, sign, submit, or prove that a transfer will succeed. A caller that
supplies a forged expected pair can still produce a misleading local preview;
on-chain account constraints remain the execution boundary.

From the repository root, run:

```sh
node --test sdk/beam-creator-recipient.test.mjs
```
