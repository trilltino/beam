# Caller-asserted balance concentration

`summarizeObservedConcentration(snapshot, context)` is a read-only local
indicator. It accepts explicit per-address token balances and supply in raw
integer units. `context.topK` and `context.maxSlotLag` must be supplied by the
caller; neither is a production threshold or trading rule. Slot lag is a
difference in Solana slot numbers, not wall-clock freshness.

Every balance carries the same mint, slot and block hash as the snapshot.
The caller also supplies an expected source ID, block hash and observation
slot. Duplicate addresses, mixed forks, stale or future slot claims, malformed
integers, and observed balances exceeding stated supply reject. These are
internal consistency checks; the caller can still lie about every input.

The output reports exact rational shares (`numerator` and `denominator`
decimal strings) for the largest K **observed addresses** and for all known
observed balances. A zero stated supply yields `null` shares. Unobserved
supply remains explicit. Results are detached and frozen, labeled
`kind: 'caller-asserted-observation'` and `verifiedChain: false` even when a
caller claims finalized data.

`addressKind: 'token_account'` counts token accounts, which can split one
owner's balance. `addressKind: 'claimed_owner'` relies on a caller's grouping
of accounts; this module does not authenticate ownership or infer common
control. Neither output identifies bots, insiders, coordinated wallets, or
safe launches. It cannot block launches or trades. No RPC, wallet, signature,
transaction submission, or external provider is used.

Before a live display, a separate trusted source must prove supply, token
account ownership/balances, block/root identity, finality and rollback
handling, and state whether the observed address set is exhaustive. None of
those proofs is implemented here.

Run `node --test backend/launch-risk/concentration.test.mjs` from the repo root.
