# Launch-risk research fixtures

Run `node --test tests/launch-risk/*.test.mjs` from the repository root. This
suite is deterministic and offline. It does not create wallets, submit orders,
identify bots, choose launch parameters, or enable a production rule.

`model.mjs` mirrors the retained pure buy/sell quote equations in
`programs/beam/src/lib.rs` (`buy_quote` and `BondingCurve::sell_quote`) using
integer reserves. The model omits every fee, slippage limit, transaction
constraint, creator claim, mint/account validation, graduation, and wallet
balance. Its `nominalSpotCapProxy` is an illustrative virtual-reserve ratio,
**not** Beam's displayed market cap or spendable pool liquidity. `realQuote`
starts at zero in the native-SOL create path. A holder's exit is bounded by
that actual reserve, while the virtual ratio may already look positive.

The tests compare fixed token quantities at matched nominal spot ratios and
different reserve depths and at one equal budget. Deeper virtual reserves
reduce price impact and can make a larger quantity affordable at the same
budget; raising the quote ratio charges
honest and coordinated buyers alike. Sequential first buys remain cheaper
whether submitted by different wallets or grouped into one slot. The model
does not model validator scheduling or detect bundles. [Jito documents
same-slot atomic bundles](https://docs.jito.wtf/lowlatencytxnsend/), while
[Solana documents transaction fees and scheduling](https://solana.com/docs/core/fees).
Blocking one submission route therefore cannot establish fair ordering.

`proRataBatch` is an **unselected comparison candidate**: capital-proportional
allocation at an illustrative common price. It is order independent and gives
no per-wallet bonus, so splitting one budget does not improve its exact
rational claim. Integer floors can alter dust distribution; the test includes
a case where splitting produces **more** unallocated dust. This is not a
settlement design: it defines no deposits, limits, refunds, oversubscription,
deadlines, censorship remedy, duplicate-claim prevention, or liveness rule.
A commit/reveal entry alternative likewise requires explicit binding of
launch, wallet, amount/limit and salt, plus nonreveal/refund policy. Neither
candidate is selected or activated.

`batch.mjs` extends that comparison with a bounded, **nonexecutable**
synthetic batch ledger. Its inputs are abstract token units, a quote target,
test-only budget labels and slots. It has no on-chain escrow or fund custody.
It reports exact rational token entitlements and quote spend/refunds; their
integer floors are diagnostics, not claimable transfers. In oversubscription,
the exact rational spending sums to the quote target and the remainder is a
refund. In undersubscription, the quote target remains partly unmet and
unsold tokens stay unallocated. The plan separates exact unsold token and
unmet quote amounts from fractional floor remainders, and reports total
integer unallocated units. **No rounding
recipient or executable settlement is selected.** Splitting one budget
preserves its exact rational share, but floor claims can lose units and
rounding can alter who would owe or receive integer quote units. A sorted
test label settles ties without a per-wallet bonus; it is not an identity
check. After a synthetic deadline, a stalled open batch can be cancelled and
each mock claimant can request its full budget once along one in-memory state
chain. The rational model rejects reuse of the same consumed state object,
but separate restored histories can still fork; there is no durable replay
protection, cross-process coordination or actual refund. This
only demonstrates a required liveness branch.
The implementation uses arbitrary-precision `BigInt`; accepting u64 inputs
does not prove u128 or on-chain arithmetic bounds for a real batch program.

The original rational illustration uses two 112-unit budgets and a chosen
224-unit target to give each participant 100 tokens. On the continuous curve,
the first 112-unit budget buys 100 while the next buys 82. Thus that rational
illustration sells **200 versus 182** tokens and is not an aggregate-economics
match. The exact-integer comparison below holds aggregate tokens and quote
spending equal. Neither chooses Beam's price, deposits or refunds.

`integer-batch.mjs` is a separate **exact-integer offline reference**. Test
inputs explicitly set token lot size, quote units per lot, lot capacity,
order budgets and slot deadlines. Demand is `floor(budget / lotPrice)`.
When total demand exceeds capacity, each order receives
`floor(capacity * demand / totalDemand)` lots; otherwise it receives its
demand. Spent quote is `allocatedLots * lotPrice` and every remaining budget
unit is a mock refund. Unallocated lots are retained, separated into unmet
demand and pro-rata rounding; no wallet is awarded the last lot. The source
tests check exact lot/quote conservation, deterministic order and tie handling,
and 3,584 bounded split-budget scenarios. Splitting cannot increase a
participant's combined allocated lots at fixed total budget and competing
orders, although it can reduce its allocation. Wealthy actors can still
dominate a common-price batch; this is not an identity or fairness guarantee.
The split non-gain property also has a bounded arithmetic argument: for price
`p`, splitting budget `B` gives total demand `sum(floor(b_i/p)) <= floor(B/p)`.
With other demand `H` and lot cap `C`, the whole-order allocation
`A(d) = d` when `d+H <= C`, otherwise `floor(C*d/(d+H))`, is monotone in `d`.
The sum of floored split allocations is at most `A(sum(d_i))`, hence at most
`A(floor(B/p))`. This assumes unchanged price, capacity, other demand and
one batch; it proves no extra **lots**, not equal fills or actor identity.

For equal 112-unit budgets, the continuous fixture sells 100 tokens to the
first buyer and 82 to the second, taking 224 quote units. A test-only batch
with two 91-token lots at 112 quote units each also sells 182 tokens for 224
quote units, allocating 91 to each. The lot size and price were fitted to
this fixture; its final reserve state, later trades, latency and price
discovery are not modeled as equivalent. An adversarial rounding case has a
90-unit order receive 4 of 5 lots and a 10-unit honest order receive zero
with its full 10-unit budget refunded. The fifth lot remains unassigned.
This is a real participation cost of the candidate's no-dust-award rule,
even though no user loses quote in this model. Batch entry also adds waiting,
cancellation and censorship costs.

The reference caps input at 128 orders and its journal at 129 events. Replay
work grows with journal length and the demonstration store replays on each
commit, so a full sequence costs more than linear work. These are local
resource bounds, not on-chain compute proofs or production caps.

The reference journal serializes a plan digest and hash-chained events. A
local in-memory compare-and-swap check rejects stale heads and duplicate
claims; replaying a JSON journal reconstructs the same state. This models
the checks a durable backend would need. Hashes do not authenticate a chain
source or prevent two independent processes from committing competing forks:
that requires atomic durable storage, which is absent. A stalled batch can be
marked cancelled after its test deadline, and mock claim vouchers show full
budget refunds. **No funds are held, sent, refunded or guaranteed to be
recoverable.** No commit/reveal, censorship remedy, fee accounting, creator
priority, direct CPI guard, LP policy or production parameter is implemented.

Before any launch rule is chosen, run adversarial comparisons at equal budgets
for creator-first access, same-slot multi-transaction bundles, split wallets,
delayed sniping, fee-switch timing, direct CPI/router bypass, buy-then-dump,
batch rounding/refunds, nonreveal/stall and graduation reserve conservation.
Report honest-user fill rates/effective prices, attacker supply share and
exit proceeds, actual reserve depth, latency and false positives. A graph of
shared funders is an uncertain signal, especially for exchange-origin funds;
it must not automatically punish users. The separate
`backend/launch-risk/` indicator only reports caller-supplied address balances,
not beneficial ownership or authenticated chain truth.

AMM v4 migration and LP custody remain separate unsolved gates. [Raydium's AMM
v4 math](https://docs.raydium.io/products/amm-v4/math) describes a constant
product pool; [Raydium states Burn & Earn does not support AMM
v4](https://docs.raydium.io/user-flows/burn-and-earn). No test here validates
post-graduation trading, deployed Raydium code, or a promised price outcome.
