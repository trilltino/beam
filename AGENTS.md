# Working on Beam

These instructions apply throughout Beam, including the archived copy at
`upstream/pumpfun-source`. That copy also has its own `AGENTS.md`; follow its
additional archive restrictions. Do not alter the pinned archive for Beam work.

## Start with the current work

As of the 1 November 2026 maintainer check, Beam's Slop listing was paused
with a security scan queued. Its current state was not verifiable on
2 November 2026. The maintainer has since explicitly resumed Beam development
in the cloud workspace; this does **not** establish Slop clearance or make a
listed task available. Check current PR ownership and maintainer direction
before work. Neither a Slop state change nor automatic task closure approves
a GitHub PR.

1. Read `README.md`, `docs/STATUS.md`, `docs/ECONOMICS.md`, `docs/TASKS.md`,
   and `CONTRIBUTING.md`. Check the working tree, remote branches, and open
   pull requests before choosing a task. Inspect the intended target branch.
2. Use `programs/beam` as the editable Anchor program. Its retained Pump Main
   modules, Beam policy/economics/Raydium modules, and compatibility tests are
   the contract baseline. Do not start a second launchpad implementation.
3. Claim one bounded lane after checking its open PRs. Record scope, base
   commit, intended behavior, tests, and remaining risks in the PR description.
   Check `docs/TASKS.md` and active pull requests to avoid duplicate work.
   External contributors can follow the [fork-to-PR guide](CONTRIBUTING.md#contribute-from-a-fork)
   without direct write access to Beam.
4. Work in a separate branch. Do not rewrite another contributor's branch or
   merge competing architectures or fee policies without a reviewed decision.
5. For website and backend work, choose a bounded lane in `docs/TASKS.md`.
   Define the interface, data source, authorization boundary, acceptance
   behavior, and local tests before enabling an external action. Use public
   behavior/docs for compatibility research, not private source or credential
   collection.

## Source and behavior boundaries

- `upstream/pumpfun-source` pins the community reconstruction at
  `5703df02131ab944408e0cf679b4bef9c22e1a3d`. Its MIT and third-party
  notices stay with the source. `upstream/raydium-cp-swap` pins reference
  Raydium CPMM files at `3d69122790e97cd6aff24e8ba95940793773f58c`.
  Treat both as reference snapshots; edit the Beam program instead.
- `scripts/check_fork.py` verifies archive Git blobs and the declared Beam
  source delta. Document every intentional change to retained Pump source.
  Source equality, host tests, and historical reconstruction receipts do not
  prove Beam's binary identity or full runtime parity with Pump.
- Mayhem creation and enablement are blocked. Legacy PumpSwap migration,
  signed router, and AMM-dependent CTO handlers fail closed. Do not reconnect
  them to PumpSwap or claim that the retained quote-chain code routes through
  Raydium.
- The existing Raydium CPMM migration is a gated, superseded candidate. The
  user chose Raydium AMM v4; build and verify that adapter separately before
  activation. Current LP burning does not leave Beam an LP fee claim for
  recurring buybacks. Principal custody and fee rights remain unresolved.
- Durable liquidity is a product goal, not a consequence proved by choosing
  AMM v4. Compare initial and continuing depth, LP ownership/control and
  withdrawal rights, lock/burn tradeoffs, slippage at comparable trade sizes,
  and post-graduation pool behavior. Do not pick an LP scheme or promise chart
  resilience from historical Raydium-versus-PumpSwap anecdotes; comparable
  liquidity and market-period evidence is required to test that hypothesis.
- `beam_economics.rs` contains provisional fee-allocation arithmetic; the
  inherited Pump fee CPI still governs real trade collection. The community
  share's holder-reward/buyback split is a provisional test setting. Holder
  eligibility, the flagship mint, vaults, and distribution rules are open.
  The user's Beam fee split is 42/5/3 nominal basis points for community,
  treasury and founder. Preserve the creator payout and creator-selected
  wallet system. Whether its fee comes from the community share or an
  additional charge remains undecided; do not assume either. Disclose each
  applicable curve, creator, migration, and Raydium pool fee; never promise returns.
- On-chain reconstruction does not provide Pump's website, streaming, callout,
  or charity service backends. Treat those as independent feature work with
  authorized providers; never imply their behavior is byte-identical. The
  pinned archive does contain separate donation-relay and Pump Fees programs;
  their presence is not Beam integration or charity authorization.
- Preserve the desired ability to pair with arbitrary compatible Solana token
  mints and supported contracts, subject to token-program, venue, account and
  security validation. Do not claim every Solana program is a valid pair or
  that general pairing is already implemented. Raydium AMM v4 remains the
  selected graduation venue. Stocks, tokenized stocks and stock-linked pairs
  are outside current scope; no future stock work is scheduled.
- Charity is an open redesign decision. Keep the archived donation source and
  existing capability evidence intact. Do not implement compulsory charity
  deductions, choose a provider/recipient, or assign a new fee share without
  a reviewed product decision. A clearly disclosed optional approach may be
  considered later; no live donation is authorized.
- Seek newcomer clarity and transparent risks. Investigate misleading copycat
  launches, fragmented liquidity across similar narratives, and extraction
  incentives without asserting competitor allegations as fact. Holding-friendly
  design is a goal, not a return guarantee. Do not impose ticker exclusivity,
  launch restrictions, fees or rewards from this direction alone, or promise
  token performance or loss prevention.
- Treat launch concentration and bundled buying as a first-class, planned
  risk gate. Displayed market cap is not reserve depth or realizable exit
  value. Use deterministic model tests for shallow reserves, concentrated
  holdings, bundled early buys and coordinated exits, with source/freshness
  limits visible. Do not infer bot identity, promise sniper prevention or
  price support, or impose thresholds, wallet caps, fees or restrictions
  without a reviewed decision. See [launch-risk research](docs/LAUNCH_RISK.md)
  and [model test scope](tests/launch-risk/README.md).
- Follow the [Beam visual identity brief](docs/BRAND.md) for the crossbar-free
  curved A mark and restrained 1980s retro-futurist frontend: black primary,
  electric blue secondary and purple tertiary. Preserve accessible data and
  controls, local fonts, mobile layout and reduced-motion behavior. Theme work
  does not activate any product feature or financial mechanism.
- King of the Hill is planned discovery, not an implemented or proven old-Pump
  clone. Verify historical rule and threshold evidence before parity claims.
  Define eligible tokens and curve stages, ranking metric and tie-breaks,
  dethroning/refresh, graduation, data freshness/reorg handling and defenses
  against manipulated inputs. Show an accessible, mobile retro featured area
  only with clearly labeled sample data or authenticated live inputs. No paid
  placement, invented leader, artificial migration delay, investment advice,
  audited-safety implication or return guarantee follows from ranking.
- Keep retro presentation tied to Beam's verified behavior, not a visual wrap
  around PumpSwap. A plain-language "How it works" onboarding explanation
  should use evidenced Beam configuration for graduation, LP custody/burn,
  fees, launch allocations and availability. Do not copy historical threshold
  numbers or assert no presale/team allocation, instant selling or LP burning
  from a third-party screenshot.
- Keep community discovery simple and give ongoing token communities a fair
  chance to remain visible. Per-token comments or opt-in social features need
  moderation, rate limits and anti-spam checks. Never use fake volume, wash
  trades, automated buy bumps or raid spam to influence ranking or engagement;
  historical status or follower counts alone do not establish legitimacy.
  Preserve planned callouts without implying paid promotion is active. Disclose
  a reward or airdrop's eligibility, funding and timing only after a reviewed
  decision; the $BEAM flagship goal is not a promise to other communities.
- The flagship $BEAM token is intended to launch on Beam, with a very high but
  unquantified share of eligible protocol revenue intended for buybacks and
  burns. No rate, mint, revenue denominator, budget, vault, authority or
  automatic execution is approved. Define actually collected, spendable Beam
  revenue separately from volume, creator entitlements, third-party fees and
  reserved community rewards. Reconcile the goal with 42/5/3 bps and creator
  funding; never double-count funds or overwrite the provisional holder and
  buyback split. A claimed higher share than competitors, including Pump.fun,
  needs comparable bases, periods and realized spending evidence. Do not
  promise price appreciation or describe any buyback as active.
- Support for communities around existing launched tokens is a separate goal
  from flagship $BEAM buybacks. Keep per-token rewards/buybacks, creator fees,
  operating allocations and any promotional spending in distinct, auditable
  budgets from realized receipts. Never count one receipt twice. The balance
  and any promotional policy are unapproved; do not ban callouts, authorize
  new spending, or promise token survival or returns from this goal.

## Validation and review

Run these from the repository root for contract changes:

```sh
python3 scripts/check_fork.py
cargo test --locked --manifest-path programs/beam/Cargo.toml
cargo test --locked --manifest-path programs/beam/Cargo.toml --features experimental-raydium
cargo check --locked --manifest-path programs/beam/Cargo.toml --no-default-features
```

Use pinned lockfiles and inspect setup hooks before installing dependencies.
Review source changes, test output, and the account/fee effects in the pull
request. Host checks do not prove Solana CPI behavior, transaction rollback,
replay resistance, or production safety. Migration and fee-rights changes need
local-validator/SBF evidence and independent review before activation.

Host tests, mock accounts, and local simulations are normal contributor work.
Do not deploy to a live network, use real wallets, sign live transactions,
trade, move funds, collect secrets, or launch live services in a pull request.
A release requires a separate maintainer-controlled process. Respect branch
protections and leave unresolved or untested work in draft state. Before any
merge, follow the [maintainer review sequence](CONTRIBUTING.md#maintainer-review-and-merge)
on the exact head and combined target tree, and satisfy required formal
approvals. Do not self-approve or bypass repository rules.

If Beam is registered on Slop, report a suspected exploitable issue through
Slop's current private security-report workflow. Otherwise use GitHub private
vulnerability reporting if available. If neither private route exists, wait
for a private maintainer route. Never publish findings or sensitive data in a
public issue or pull request.
