# AMM v4: reconciling the open PRs, rights, quotes and routing

State as of 3 November 2026, against upstream `main` at `0a3ba8d`. "Read" below means the code was
read; "run" means it was executed. Several drafts could only be read, because they sit on other draft
branches.

## 1. Dependency and ownership map

All of #29, #30, #33, #34, #59 and #63 are stacked drafts: each is based on the previous draft's branch
and none on `main`, so none can merge on its own. #49 is stacked on #36, which conflicts with `main`.

| PR | State | Scope (read) | Overlap with the migration branch | Decision |
| --- | --- | --- | --- | --- |
| #29 | Draft, base `codex/beam-amm-v4-config-guard` | `beam_amm_v4.rs` +487: a native-SOL source-account preflight over the 19 `Initialize2` accounts | Source checks, mint-decimals guard | **Adapted.** The 18-decimal guard and the initialized/unfrozen/no-delegate/no-close-authority source checks are in. Its exact-balance rule is not: it lets anyone block graduation by donating one unit |
| #30 | Draft, base #29 | `beam_amm_v4_funding.rs` +389: a disabled funding planner (`plan`, `settle`), not imported | Duplicates `setup_cost` and `migration_plan`, which run on a real CPI | **Omitted** (duplicate) |
| #33 | Draft, base `codex/beam-runtime-retired` | `beam_amm_v4_settlement.rs` and 2,856 lines of synthetic-bank tests | Post-CPI assertions, now done by `verify_pool` against the real processor | **Omitted**; synthetic mocks do not exercise Raydium |
| #34 | Draft, base #33 | `beam_amm_v4_snapshot.rs` (776): a source-pinned decoder of the pool and token accounts | Post-CPI snapshot | **Adapted.** The same packed offsets are in `beam_amm_v4_pool_state.rs`, pinned by tests. Its alias and exact-equality rules are not adopted. If maintainers prefer #34's module, `PoolState::verify` is the one swap point |
| #36 | Draft, base `main`, **conflicts** | Rejects PumpSwap quote pools and CPI bodies (`create_v2`, `migrate*`, `multi_hop`, `pump_quote`, `beam_pumpswap`) | None with the migration module | **Untouched.** Prerequisite of #49 |
| #39 | Draft, base `main`, mergeable | Anchor IDL generator, IDL, PDA sidecar, compatibility tests | The IDL lacked the new instructions | **Merged into the IDL/SDK branch** and extended |
| #49 | Draft, base #36's branch | 33 files, +12,719/-3,418: rights, creator operations, vaults, router, quote | Rights type, quote reader | **Not merged.** Interop is kept byte-compatible (section 2) |
| #59 | Draft, base #34 | Synthetic tests of #34's guards | Tests of a module not adopted | **Omitted** |
| #63 | Draft, base #59 | `beam_amm_v4_probe.rs`, a host-only CPI diagnostic, and 650 lines of tests | Shows Beam can sign `Initialize2` | **Omitted**; the real migration supersedes it |

Related and newer: #47 and #64 (merged) added `tests/legacy-trade-runtime`; the branch merges with
`main` cleanly and that job passes. #45 and #46 (ready for review, other contributors) add
dependency-free PDA/ATA derivation and curve quote helpers; `beam-amm-v4-addresses.mjs` duplicates the
derivation and should use #45's when it merges. Merging #49 into this branch conflicts in `lib.rs` and
`upstream/manifest.json` and, because it sits on #36, in seven further files (the README, five docs and a
workflow) that already conflict with `main`.

## 2. One canonical rights type

| Property | `BeamAmmV4Rights` (this branch) | `AmmV4Rights` (#49, `32c661d`) |
| --- | --- | --- |
| Discriminator | `[88, 155, 79, 159, 221, 232, 159, 98]` | `sha256("account:AmmV4Rights")[..8]`, the same |
| Fields and order | pool, mint, market_seed, creator, creator_payout, donation_record, revision, paused | identical |
| Size | 209 bytes | identical |
| PDA | `["beam-amm-rights", pool]` | identical |
| Created by | `migrate_amm_v4`, permissionless; values come from the curve | `register`, needs the Beam authority and the creator |
| Updated by | nothing here; write-once | `change_creator`, `takeover`, `pause`, donation selection |

Both are checked byte for byte (`assert_rights_record`; the SDK fixture). The final repository should
have **one** type: when #49 lands, delete `BeamAmmV4Rights` and use its `AmmV4Rights`. #49's `register`
creates the account with `init`, so for a migrated pool it fails with an account-already-in-use error;
it cannot overwrite or hijack a migration-created record. The creator-payout default and the empty
donation selection are #49's proposal, not an approved design.

## 3. Quotes

**Checked against migrated pools** (run): every hard-coded offset in #49's reader against the packed
`AmmInfo`; `SwapOnly` status; pool, vault, mint, LP-mint, seed and target-orders identities; vault
ownership with no delegate or close authority; WSOL backing; zero pending PnL; the rights record
bytes. **Not run:** #49's quote function itself, which lives on a branch that cannot merge here.

**Manipulation (read, not tested).** The reader returns spot vault reserves minus pending PnL. In
`create_v2.rs`, `validate_quote_mint` returns a quote record that `init_curve` uses for a new launch's
initial reserves. A graduated token used as the quote asset therefore sets a new curve's starting
price from a ratio that one transaction can move: swap in the pool, create the launch, swap back.
AMM v4 exposes no time-weighted price, so a spot ratio is not an oracle and no bound is derivable
from the pool alone. Until the maintainer picks a bound (a reference price written by an authority,
a cap relative to such a price, or refusing graduated quotes), keep that path fail-closed, as PR #12
leaves it. No other financial operation in the migration branch reads a pool-derived price.

## 4. Routing boundaries

| Operation | Behavior | Notes |
| --- | --- | --- |
| Buy and sell on the bonding curve | Stop once the curve is complete | Existing behavior, unchanged |
| Graduation | `migrate_amm_v4`, gated | Disabled in every production build |
| Swaps on a graduated token | Happen on Raydium directly | Beam has no AMM v4 router |
| AMM v4 pool as a quote asset | Fail-closed | Needs an approved manipulation bound (section 3) |
| Legacy PumpSwap `migrate`, `migrate_v2`, router, `admin_cto` | Fail-closed | Preserved; replacement belongs to #36 and #49 |
| Mayhem | Blocked | Unchanged |
| Superseded CPMM migration | Fail-closed | Unchanged; a test now pins this |

These rows describe the existing code and documents; this work changes none of them.

## 5. Application boundary

The frontend lifecycle display is already scoped by drafts #31 and #32 ("Model unavailable AMM v4
lifecycle" and "Show disabled AMM v4 migration status"), and the indexer by #16, #58 and #60, which
are fixture-only. This work does not edit `web/` or the indexer, to avoid duplicating them. It
supplies the data contract they can consume: `sdk/beam-amm-v4-state.mjs` reads the accounts through an
injected `getAccount` and classifies `CurveActive`, `CurveComplete`, `MigrationUnavailable`,
`MigrationEligible`, `MigrationPending`, `MigrationConfirmed`, `MigrationFailed` and `Graduated`.
Graduation requires the receipt, the rights record and a Raydium-owned pool to agree with one another
and with the addresses derived from the mint; a transaction status or a logged event alone is never
enough. An indexer needs: signature, slot, commitment, mint, curve, pool, LP mint, amounts, fees,
receipt and rights, and must treat events as hints that a finalized account read confirms.

**Not done:** UI wiring, a migration button, and live indexer ingestion. A production UI must not
offer a migration while the program rejects activation.
