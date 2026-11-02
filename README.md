# Beam

Beam is an independent development fork of a community reconstruction of
Pump's on-chain launchpad. The intended venue after the bonding curve is
Raydium AMM v4; Mayhem is disabled, and finished Beam must have no operational
PumpSwap fallback. The goal is to preserve other observable launchpad behavior
where the venue and independently built services permit it.

**Current result:** an undeployed Anchor source fork, local ABI fixtures, and a
sample-data website preview. Beam is not audited or ready for live use. The
superseded Raydium CPMM migration is blocked in every build. AMM v4 has
source-level guards, draft synthetic Beam tests, and a merged local test that
invokes the pinned Raydium source processor. There is no Beam migration
entrypoint or deployed-program validation. The modified Beam binary cannot be
byte-identical to Pump's binary.

## Product direction

Beam aims to let users pair with arbitrary **compatible** Solana token mints
and supported contracts after token-program, venue, account, and security
validation. This is a desired capability, not implemented support for every
Solana program or trading pair. Raydium AMM v4 remains the selected graduation
venue and has its own mint compatibility limits.

Stocks, tokenized stocks, and stock-linked pairings are outside current scope.
Possible reconsideration has no committed plan or date. Charity support is an
open redesign decision: the archived donation source stays intact, while no
compulsory deduction, provider, recipient, or new allocation has been approved.
A different, clearly disclosed optional approach remains possible.

The product goal is a clearer, more transparent experience for newcomers and
more reason to hold through honest utility and market design. Investigate
misleading copycat launches, narrative-driven liquidity fragmentation, and
incentives for opportunistic extraction. These concerns do not authorize
ticker monopolies, launch restrictions, new fees, or holding rewards. Beam
does not promise profitable launches, price support, loss prevention, or any
particular token size. See the [feature ledger](docs/FEATURES.md) for what is
actually implemented.

Launch concentration and bundled early buying are a planned risk gate, not a
working restriction or bot detector. A displayed market cap does not measure
the reserve depth available for a sale. Before showing a risk label or changing
launch behavior, test reserve depth, concentrated holdings, bundled buys and
exit price impact with deterministic cases and stated data limits. No wallet
cap, threshold, anti-sniper protection or price outcome has been approved.
See the [launch-risk threat model](docs/LAUNCH_RISK.md) and
[local model tests](tests/launch-risk/README.md).

The flagship **$BEAM** token is intended to launch on Beam. The goal is to
devote a very high, **not yet quantified** share of eligible protocol revenue
to $BEAM buybacks and burns. No percentage, revenue denominator, spending
authority, mint, or execution budget is approved. Define revenue from amounts
Beam actually collects and may spend; do not treat trading volume, creator
entitlements, third-party Raydium fees, or reserved community rewards as
available buyback funds. Reconcile this goal with the existing 42/5/3-bps
allocation and unresolved creator funding before implementing it. An ambition
to allocate more revenue than competitors, including Pump.fun, is unverified
until comparable bases, periods, commitments and realized spending are checked.
No buyback or burn is active, and neither guarantees $BEAM price appreciation.

Support for communities around existing launched tokens is a separate goal
from flagship-only $BEAM spending. Any per-token rewards or buybacks, creator
payouts, operations and promotional spending need distinct, auditable budgets
from actually collected receipts; the same revenue cannot fund two promises.
The balance among them remains undecided. Callouts remain a planned feature,
but no promotional spend is approved, and neither attention nor fee recycling
can guarantee a token's survival or returns.

The [visual identity brief](docs/BRAND.md) specifies a crossbar-free, curved
capital A mark and a restrained 1980s retro-futurist frontend. Black remains
primary, electric blue secondary and purple tertiary. The current website is
still a sample-data preview.

**King of the Hill** is a planned token-discovery area with a visible featured
leader in the retro UI. Historical eligibility, ranking, turnover, and
graduation rules need evidence before any old-Pump parity claim. The preview
has no authenticated live leader or paid placement. A ranking would be a
discovery signal, not investment advice, a safety audit, or a return promise.
No artificial graduation delay is planned.

Discovery should stay simple and give established token communities room to
remain visible. Per-token discussion and opt-in community tools are ideas for
future work, with moderation and spam limits required. Honest user activity
must not be confused with automated buy bumps, wash volume or coordinated raid
spam. Any reward or airdrop must disclose its eligibility, funding and timing
only when those terms are actually decided; none is promised here. The $BEAM
flagship goal does not promise rewards to every launched-token community.

Durable liquidity is also a product goal. Future validation must compare
initial and continuing pool depth, LP ownership and withdrawal control,
lock-versus-burn tradeoffs, slippage at comparable trade sizes, and behavior
after graduation. The historical claim that Raydium charts were more durable
than PumpSwap charts is an unverified hypothesis, not proof of venue causation
or a promise that Beam charts will be resilient. No LP custody or burn design
is chosen here.

## Attribution / Upstream

Beam vendors the community reconstruction of pump.fun's on-chain programs as an [archived pumpfun-source snapshot](upstream/pumpfun-source/README.md), pinned at commit `5703df02131ab944408e0cf679b4bef9c22e1a3d`. The [provenance record](docs/UPSTREAM.md) includes the historical repository URL.

The reconstruction is a community publication, not an official pump.fun release. The archived MIT notice names "pump.fun contributors." Beam preserves that notice with the source.

**Upstream availability (2 November 2026):** The historical public repository
and pinned commit URLs return 404. We cannot tell whether the repository was
deleted, made private, or moved. Beam's [local archive](upstream/pumpfun-source/README.md)
preserves all 380 files in the pinned source-only snapshot, verified by path,
mode, and Git blob hash. It is not a backup of all Git history, research
artifacts, or GitHub discussions.

`upstream/pumpfun-source` is that commit. `python3 scripts/check_fork.py` checks its 380 Git blobs against `upstream/files.json`. In the editable program `programs/beam`, the same command compares Pump Main (`programs/pump` at that commit) byte for byte: 62 retained files match, and four retained files are declared Beam edits. Five more files exist only in Beam. The whole `programs/beam` crate is that fork. Source byte identity is not a compiled Beam binary, a deployed pump.fun bytecode match, or runtime parity. Beam does not re-prove the reconstruction against deployed bytecode.

The retained-file list, the declared edits, and the preserved license notices are in [Upstream attribution](docs/UPSTREAM.md). This repository has no root `LICENSE`. The archived notices stay with the snapshot. These documents do not add or choose a license.

## Contribution focus

Choose one bounded gap from the [task ledger](docs/TASKS.md), check open PRs for
overlap, and record the base commit, affected paths, tests, and remaining risks.
The [contributor guide](CONTRIBUTING.md) gives commands and the maintainer
review sequence, including [fork-to-PR steps](CONTRIBUTING.md#contribute-from-a-fork)
for people without direct repository write access. A passing mock or host test
is evidence for that narrow case, not proof of a deployable launchpad.

At the 1 November 2026 maintainer check, Beam's
[Slop listing](https://slop.cash/projects/beam) was paused with a security
scan queued. Its current status could not be verified on 2 November. Confirm
current maintainer direction before treating a listed task as available or
resuming feature work. A Slop task's automatic closure does not approve a
GitHub PR.

## Progress at a glance

This is a **development** snapshot from 2 November 2026. **Merged** means
source is on `main`, **Draft** means it is in an unmerged PR, and **Blocked**
means a required implementation or decision is missing. These labels are not
completion percentages or production approval.

### Contract and source compatibility

| Goal | State | Evidence and remaining work |
| --- | --- | --- |
| Preserve the recovered Pump source | **Merged source** | The pinned archive passes [380 Git-blob checks](docs/VALIDATION.md). Of the retained Pump package, [62 files are byte-identical and four changes are declared](docs/UPSTREAM.md#what-is-byte-identical). This is source evidence, not Beam binary identity. |
| Launch tokens and trade on the curve | **Merged development code** | [PR #1](https://github.com/shawkkkkk/beam/pull/1) contains Anchor handlers and selected host tests. Complete instruction execution and SBF behavior remain unverified. |
| Remove Mayhem and PumpSwap | **In progress** | Mayhem creation and legacy PumpSwap entry paths fail closed; [PR #12](https://github.com/shawkkkkk/beam/pull/12) blocked a nested graduated quote. Router, creator/sharing, CTO and donation dependencies still need compatible replacements. |
| Graduate to Raydium AMM v4 | **Blocked; partial test evidence** | [Merged PR #9](https://github.com/shawkkkkk/beam/pull/9) adds source-only ABI guards. [Merged PR #38](https://github.com/shawkkkkk/beam/pull/38) invokes the pinned Raydium source processor through a local synthetic probe. Draft [#22 → #29 → #30](docs/TASKS.md#draft-review-queue-prs-2234) and [#25 → #33 → #34](docs/TASKS.md#draft-review-queue-prs-2234) add Beam account, funding, mock settlement and snapshot checks. No Beam migration CPI, LP-rights design or Token-2022 graduation exists. |
| Enforce Beam fees and creator payouts | **Blocked; math and disabled policy in draft** | [PR #5](https://github.com/shawkkkkk/beam/pull/5) merged provisional 42/5/3 arithmetic. Draft [#23/#24/#26](docs/TASKS.md#draft-review-queue-prs-2234) test stricter caps and disabled state. Live collection, selected creator wallet payout and post-graduation rights are unresolved. |
| Whole Beam binary matches Pump | **Different by design** | Beam changes the program identity, venue, Mayhem and fee code. Source matches in retained files do not establish a compiled or deployed byte match. |

### Website, frontend, and SDK

| Goal | State | Evidence and remaining work |
| --- | --- | --- |
| Beam-branded discovery and token pages | **Merged preview** | [PR #6](https://github.com/shawkkkkk/beam/pull/6) added a responsive UI with sample market data and browser-local drafts. It has no live chain feed. |
| King of the Hill featured leader | **Planned; no live ranking** | Historical rules and authenticated Beam inputs need verification. Define eligibility, ranking, ties, refresh/dethroning, graduation and rollback before a responsive retro UI can display a real leader. Sample leaders must be labeled. |
| Create, wallet, and trade flows | **Local SDK merged; live flow blocked** | [#7](https://github.com/shawkkkkk/beam/pull/7), [#18](https://github.com/shawkkkkk/beam/pull/18) and [#20](https://github.com/shawkkkkk/beam/pull/20) provide native-SOL/CreateV2 fixtures and raw event decoding. Draft [#13/#27](docs/TASKS.md) cover creator and V2 trade fixtures. There is no deployed Beam ID, finalized IDL, wallet signing or trade submission. |
| Creator wallet, charity, callouts, livestream screens | **Local plans in draft** | [#17](https://github.com/shawkkkkk/beam/pull/17) merged an unverified read-only data boundary; draft [#28 → #31 → #32](docs/TASKS.md#draft-review-queue-prs-2234) adds unavailable-state preview copy. These do not provide authorized donations, calls, streams or migration. |

### Backend and integrations

| Goal | State | Evidence and remaining work |
| --- | --- | --- |
| Read-only market/indexer API | **Web boundary merged; live feed blocked** | Draft [#16](https://github.com/shawkkkkk/beam/pull/16) has local replay/query fixtures; merged [#17](https://github.com/shawkkkkk/beam/pull/17) has an unverified web adapter. Their schema needs reconciliation. Neither authenticates a chain feed. |
| Creator sharing and payout service | **Blocked** | Retained on-chain source has creator paths, but Beam still depends on inherited Pump fee configuration. No Beam-controlled payout integration is verified. |
| Charity donations | **Mock draft; live use blocked** | [#14](https://github.com/shawkkkkk/beam/pull/14) provides a disabled local registry/provider mock. There is no authorized Donate.gg integration or real settlement. |
| Callouts and livestreaming | **Mock drafts; live use blocked** | [#15](https://github.com/shawkkkkk/beam/pull/15) and [#21](https://github.com/shawkkkkk/beam/pull/21) test local moderation and lifecycle behavior; no production provider or playable live stream exists. |

See [implementation status](docs/STATUS.md), [feature matrix](docs/FEATURES.md)
and the [dated PR dependency queue](docs/TASKS.md) for more detail.

## Required before live use

| Requirement | Current boundary |
| --- | --- |
| Verified AMM v4 migration and liquidity | Merged PR #38 invokes the pinned Raydium source processor through a local probe and tests selected rollback and replay cases. It does not execute Beam's migration handler or validate deployed Raydium bytecode. Authenticate Beam account state and receipts; measure pool depth, LP rights, slippage and post-graduation behavior before choosing LP custody or enabling migration. |
| Token compatibility | Raydium AMM v4 accepts classic SPL Token mints; Beam's curve also retains restricted Token-2022 support. A safe graduation plan for those launches is unresolved. |
| Enforced and disclosed fees | Beam's proposed allocation is not wired into trades. Decide creator funding, vault authority, community distribution, migration charges and all-in quote behavior. |
| Complete client and services | Regenerate the Beam IDL; add authenticated indexer, wallet/client and authorized creator, charity, callout and media integrations. Sample data and mocks cannot be presented as live observations. |
| Release evidence and authorization | Complete SBF/local-validator tests with real permitted program dependencies, independent security review and maintainer-controlled deployment decisions. This repository deploys nothing. |

## Evidence and components

| Component | Location | What its current evidence establishes |
| --- | --- | --- |
| Community Pump reconstruction | [`upstream/pumpfun-source`](upstream/pumpfun-source/README.md), pinned at [`5703df0`](https://github.com/lalalune/pumpfun-source/tree/5703df02131ab944408e0cf679b4bef9c22e1a3d) | Pristine reference archive with retained notices; upstream's historical reconstruction claim is not a Beam binary match. |
| Editable Beam contract | [`programs/beam`](programs/beam/Cargo.toml) | Anchor development code, declared source differences and selected host tests; no complete runtime parity or deployment. |
| Raydium references | Historical [CPMM `3d69122`](https://github.com/raydium-io/raydium-cp-swap/tree/3d69122790e97cd6aff24e8ba95940793773f58c); selected AMM v4 [source `91739d8`](https://github.com/raydium-io/raydium-amm/tree/91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6) | CPMM is superseded and disabled. [PR #38](https://github.com/shawkkkkk/beam/pull/38) provides local CPI evidence for the pinned AMM v4 source processor, not deployed-bytecode or Beam-handler validation. |
| Local client and UI | [`sdk/`](sdk/README.md), [`web/`](web/README.md) | ABI fixtures and sample-data preview; no wallet, RPC-authenticated market data or transaction service. |
| Backend drafts | [Task ledger](docs/TASKS.md#website-and-backend-contribution-lanes) | Local fixture indexer, charity, callout and livestream work in unmerged PRs, not services on `main`. |

Source equality, host compilation, synthetic bank tests and real CPI execution
are different evidence levels. None implies a whole-program binary match,
production safety or a promised market outcome. The historical ATH/fee
comparisons motivated Beam; they do not establish that a venue caused token
performance.

## Planned ordinary fee

| Allocation | Proposed rate | Share of 0.50% |
| --- | ---: | ---: |
| Community rewards and buybacks | 0.42% | 84% |
| Treasury | 0.05% | 10% |
| Founder | 0.03% | 6% |
| **Total** | **0.50%** | **100%** |

Treasury plus founder are 8 basis points under platform control. Creator
payout behavior and creator-selected wallet support remain requirements.
Whether creator payouts come **from** the 0.42% community share or add to the
trade charge is undecided, so 0.50% is **not** an all-in quote. A 50/50
holder-reward/buyback split is only a test assumption; beneficiaries, custody
and spending rules are not approved. Draft [#23](https://github.com/shawkkkkk/beam/pull/23)
corrects dust caps, but no live Beam collection is wired. Raydium pool and
retained migration fees are separate. See [economics](docs/ECONOMICS.md).

An optional three-second, buy-side 99%-to-base launch-fee quote is
**disabled and test-only**. Its initial 99% is modeled on gross buyer debit,
not net base; the extra charge has no approved recipient or withdrawal path.
It does not detect bots or promise a price outcome.

## Get started

Prerequisites: Git, Python 3, a Rust/Cargo toolchain, Node.js 22 for the SDK
and Node.js 24 with npm for the website. Inspect installation hooks and use
the repository lockfiles. No wallet, private key or live RPC is needed.

```sh
git clone https://github.com/shawkkkkk/beam.git
cd beam
python3 scripts/check_fork.py
cargo test --locked --manifest-path programs/beam/Cargo.toml
cargo test --locked --manifest-path programs/beam/Cargo.toml --features experimental-raydium
cargo check --locked --manifest-path programs/beam/Cargo.toml --no-default-features
node --test sdk/beam-native.test.mjs
(cd web && npm ci && npm run build && npm run test:unit && npm run format:check)
```

To view the sample-data website, run `cd web && npm run dev` after `npm ci`.
The Chromium interaction suite is separate from hosted website CI and requires
an isolated test environment; see [contributor checks](CONTRIBUTING.md#check-the-change).
The draft `tests/runtime-harness` has its own locked commands in that guide.
Build flags do not activate the disabled CPMM adapter or provide an AMM v4
migration entrypoint.

## Repository layout

- `programs/beam/`: editable Anchor contract and host compatibility tests.
- `upstream/`: pinned third-party source snapshots and notices; keep them immutable.
- `scripts/check_fork.py`: source provenance and declared-difference check.
- `sdk/`: local native-SOL ABI fixtures and decoder/builder drafts.
- `web/`: Beam-branded sample-data preview.
- `docs/`: economics, feature matrix, status, validation and contributor tasks.
- `.github/workflows/`: current hosted checks; verify which changed tests each workflow actually runs.

## Open priorities and contribution

The immediate engineering gates are authenticated AMM v4 migration, LP and
creator fee rights, Token-2022 graduation, Beam-controlled fee routing,
PumpSwap-dependent feature replacements, a final Beam client and authorized
off-chain services. They remain **blocked or in draft** while feature work is
paused; this list does not reopen them. See [task ownership](docs/TASKS.md)
before taking a lane.

Start with [CONTRIBUTING.md](CONTRIBUTING.md),
[AGENTS.md](AGENTS.md), [status](docs/STATUS.md),
[validation](docs/VALIDATION.md), and [economics](docs/ECONOMICS.md).
Use the [maintainer review checklist](CONTRIBUTING.md#maintainer-review-and-merge)
for stacked PRs and exact-head CI. Suspected exploitable issues belong in the
current private security-report channel, never a public issue or PR.

Beam has no repository-root license yet. Archived sources retain their own
licenses; maintainers must clarify terms for new Beam code before accepting
substantial external submissions.
