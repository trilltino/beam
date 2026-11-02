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
