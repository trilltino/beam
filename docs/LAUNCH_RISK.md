# Launch concentration and sniping: threat model

**Status:** research and test plan. Beam has no approved launch restriction,
wallet-linkage rule, bundling detector, anti-sniping surcharge, or automatic
enforcement. This document does not set a threshold or activate a feature.

## What this protects and what Beam currently does

The goals are clear launch terms, predictable maximum spend and minimum output,
and honest discovery for newcomers and existing token communities. An adverse
launch can concentrate supply, produce large early price moves, or leave later
buyers facing poor liquidity. These outcomes can occur without a protocol bug;
no test or policy can promise a profitable or manipulation-free token.

The retained [`create_v2`](../programs/beam/src/create_v2.rs),
[`buy`](../programs/beam/src/buy.rs), and
[`sell`](../programs/beam/src/sell.rs) paths are development source, not a live
Beam market. [`beam_economics.rs`](../programs/beam/src/beam_economics.rs)
calculates a proposed ordinary 42/5/3-basis-point allocation but is not wired
into trade collection. The optional three-second launch-fee quote described in
[economics](ECONOMICS.md) is disabled and test-only; its timing, exemptions,
recipient and deployment are **not** approved. Mayhem and legacy PumpSwap paths
fail closed in [`beam_policy.rs`](../programs/beam/src/beam_policy.rs). Raydium
AMM v4 graduation is not active. The [website](../web/src/App.tsx) displays
sample data, and the merged [web indexer boundary](../web/src/indexerAdapter.ts)
does not authenticate chain data. Neither is a bot detector.

### Evidence boundary

| Evidence | What it can establish after a trusted source and finality checks | What it cannot establish by itself |
| --- | --- | --- |
| Transaction signatures, instructions, signers, fee payers, token/lamport deltas, slot and order | Which accounts signed and what the successful transactions did. Multiple instructions in one Solana transaction commit together or revert together. | Who ultimately controls a key, why they traded, or whether separate transactions were privately coordinated. |
| Several buys in one slot or a short interval | Temporal concentration and observed order in that ledger view. | A private bundle, shared owner, bot identity, or an unfair intent. Jito documents same-slot atomic bundles as one submission path; co-slot transactions alone do not prove that path was used. |
| On-chain funding transfers between wallets | A transfer path and amount between public keys. | Common beneficial ownership. An exchange, bridge or shared service can fund unrelated customers. |
| Creator key or launch transaction | The creator recorded by the instruction and any visible same-transaction actions. | Whether a different key had advance knowledge, or that all creator-associated wallets are known. |
| Off-chain metadata, social posts, analytics scores | A claim that can be displayed with its source and timestamp. | Authenticated trading identity, token safety, future liquidity, or a guaranteed return. |

Sources: [Solana transaction structure](https://solana.com/docs/core/transactions/transaction-structure)
describes signed messages and atomic instruction execution;
[Jito's bundle documentation](https://docs.jito.wtf/lowlatencytxnsend/)
describes its same-slot, sequential bundle service. These sources do not make
every same-slot group a Jito bundle. Any chain-facing implementation also
needs a trusted network/source identity, replay and rollback handling; a
caller-supplied slot or signature is not authentication.

## Adversaries, false positives and fair-user costs

| Scenario | Observable signal to test | Uncertainty and user impact |
| --- | --- | --- |
| Creator or informed party buys before broad discovery | Creation and early buy instructions, signer keys, timing and supply acquired. | A different wallet's relationship to the creator is uncertain. A public label must describe the transactions, not accuse an individual. |
| Same-transaction or same-slot multi-buy sequence | Instruction order and account deltas in a transaction; later transactions' slot/order after finality. | Several ordinary users can share a slot. Private bundle membership is not generally proven by the ledger. A slot-only block would harm unrelated buyers. |
| One actor splits purchases across many wallets | Per-wallet transfers and aggregate distribution if the keys are *independently* linked. | Sybil linkage is probabilistic without stronger evidence. Per-wallet limits can be bypassed and can exclude legitimate separate users. |
| Wallets share a funding source | Funding graph, amounts and times. | Exchange/bridge omnibus funding creates false positives; shared source is never proof of common control. Do not publicly name an alleged controller from this heuristic. |
| Buyer waits beyond an early-launch window | Buy timing and resulting price impact. | A time-limited fee or rule can shift activity later, not remove it. Chain time/slot boundaries and quote expiry must be specified before testing a proposed rule. |
| Large holder exits or repeated sells | Executed sells, balances, liquidity depth and price impact from a verified pool. | Future selling and intent cannot be inferred. A launch guard does not make holding safe or prevent a dump. |
| Defensive fee, delay or block harms an ordinary user | Total debit, received tokens, slippage, failed transaction fees, cancellation/refund behavior and accessibility of the quote. | A high fee can consume most of a buy; restrictions can be costly or confusing. No hidden exemptions, trapped proceeds or unspendable output may be introduced. |

## Adversarial acceptance cases

These are **synthetic local cases**, not a scoring rule. A future detector or
quote experiment must pass them before a proposal for activation; tests should
use exact integer amounts, preserve original event/account provenance, and
cover reorgs and stale data.

1. A creator-key purchase in the launch transaction and a separate early buy
   are represented in their actual instruction/slot order. They are not both
   labeled a private bundle. The UI explains observed concentration without
   alleging ownership of unrelated keys.
2. Several unrelated buyers share one slot; a multi-transaction bundle-shaped
   sequence has the same visible order. A chain-only classifier does not claim
   to distinguish those cases without independent bundle evidence.
3. A single test actor uses split wallets; unrelated users receive transfers
   from the same exchange-like funder. Funding-only linkage must not classify
   the unrelated users as one owner or deny them a trade.
4. A buyer waits until after any proposed early window, then buys and exits.
   The test must show the remaining price/liquidity risk; it must not report
   that the launch mechanism prevented sniping or dumping.
5. Simulate a failed or stale quote and a dust-sized buy. Check gross debit,
   every separately applicable fee, maximum-spend/minimum-output guards,
   zero-output rejection, transaction fees on failure and rollback of any
   program-side transfers. Normal sells must not inherit a buy-only surcharge.
6. A fork replaces an unfinalized early transaction; stale rankings, labels
   and concentration summaries are invalidated. A finalized record cannot be
   silently rewritten by a caller-supplied fixture.
7. Include an ordinary user who buys near a boundary and a creator who never
   buys. Compare false positives, actual costs and clear disclosures against
   any claimed benefit. Report uncertainty instead of a binary "bot" label.

## Decision gates before any mechanism

### Candidate paths to compare

| Path | Potential benefit to test | Cost or missing decision |
| --- | --- | --- |
| Existing continuous curve | Immediate entry and retained quote behavior provide the baseline. | Ordered early buys get different prices; bundling and linked wallets remain possible. |
| Disabled opening-fee experiment | A time-limited expensive opening may discourage some immediate buys. | It also charges ordinary buyers and can move the race to the fee boundary; surcharge allocation, denominator, timing and exemptions are undecided. |
| Bounded common-price batch | [Offline integer fixtures](../tests/launch-risk/README.md) remove order advantage *inside the chosen batch* and show no lot gain from splitting one fixed budget. | A wealthy buyer still buys more; small orders can receive zero due to floor rounding. Price, lot size, delay, custody, durable claim/refund storage and direct-route enforcement are undecided. |
| Commit/reveal or delayed disclosure | Could reduce some visible advance-order advantage if all entry routes obey the same rule. | Public deposits may leak size; nonreveal, censorship, deposits, refunds and deadline recovery need a reviewed design. |

The next useful local experiment is to compare these candidates with equal
buyer budgets, aggregate token/quote totals, fair-user fill and wait costs,
including creator-first and delayed side-wallet cases. The integer fixture
holds one comparison's **aggregate** tokens and quote spent equal: a
continuous first buyer obtains 100 tokens for 112 quote units and the next
obtains 82 for 112; a separately priced two-lot batch allocates 91 to each
for the same 182 tokens and 224 quote units. That price and lot size were
chosen only to make the comparison, not approved as Beam policy. The batch
cannot establish the same later curve reserve state, nor prevent a larger
budget from dominating. No path is selected for activation.

Research must first define the attack and a measurable fair-user outcome.
Any proposed buy-side fee or restriction then requires explicit decisions on
the fee denominator (gross debit versus net notional), time/slot start and
end, applicability to all buyers, exemptions, proceeds custody/recipient,
creator treatment, maximum spend/minimum output, refunds, and failure costs.
The ordinary Beam allocation and unresolved creator payouts cannot be silently
reused to distribute a launch surcharge. A 99% headline must state its
denominator; it is not a bot-detection claim. See [economics](ECONOMICS.md).

Before activation, require source-level and local-bank adversarial tests,
independent security/economic review, privacy-aware false-positive analysis,
clear UI quotes and a separately authorized deployment decision. No wallet
blacklist, launch delay, ticker monopoly, wash-volume signal, automatic buy
bot, hidden exemption, new charge or fund movement is approved here.
