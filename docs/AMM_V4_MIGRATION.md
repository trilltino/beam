# Gated Raydium AMM v4 migration instruction

`migrate_amm_v4` in [`beam_amm_v4_migration.rs`](../programs/beam/src/beam_amm_v4_migration.rs)
is Beam's first executable AMM v4 graduation path. It is **disabled in every
production build**. It exists so the Beam-to-Raydium path can be exercised
against the pinned Raydium processor while the product decisions below stay open.

This page covers one bounded lane. It does not choose LP custody, fee funding or
a Token-2022 plan, and it does not touch `upstream/`. Draft PRs #29, #30, #33,
#34, #59 and #63 build funding, settlement and snapshot helpers for the same
destination; this instruction deliberately carries its own small plan and
post-CPI checks so it can be reviewed alone. Reconcile with that stack before
merging either side, and fold this page into `AMM_V4_INTEGRATION.md` when that
file lands.

- Raydium source pin: `raydium-io/raydium-amm@91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6`
  (also the upstream default-branch head on 2 November 2026).
- ABI: the 19-account `Initialize2` path in [`beam_amm_v4.rs`](../programs/beam/src/beam_amm_v4.rs).
  Raydium's own `instruction.rs` helper still builds an older 21-account form;
  the processor accepts both and Beam sends the 19-account one.
- Anchor stays at 0.31.1. The instruction calls Raydium with `invoke_signed`
  through Beam's small source-pinned adapter, not through `raydium-cpi`, whose
  current `master` targets Anchor 1.x.

## Instructions

| Instruction | Who | Effect |
| --- | --- | --- |
| `configure_amm_v4(max_create_pool_fee)` | `Global.authority` | Creates the `beam-amm-v4-config` PDA with `enabled = false` and the largest Raydium create-pool fee Beam will pay. |
| `set_amm_v4_enabled(enabled)` | `Global.authority` | `false` always works. `true` is rejected unless the program was built with `experimental-amm-v4`, which no release uses. |
| `migrate_amm_v4(open_time, max_create_pool_fee, setup_budget)` | anyone, paying | Graduates one completed curve and writes its rights record. Requires `config.enabled` and the inherited `Global.enable_migrate`. |

`max_create_pool_fee` must not exceed the stored cap, and Raydium's *current*
fee, read from its canonical config immediately before the CPI, must not exceed
`max_create_pool_fee`.

`setup_budget` is bounded on both sides. Beam computes the exact setup cost: the
create-pool fee plus rent for the six accounts `Initialize2` creates (pool,
target orders, LP mint, two vaults, LP account). The budget must be at least that
cost for the current fee and at most that cost at `max_create_pool_fee`
(`SetupFundingOutOfBounds` otherwise). After the CPI, Beam requires Raydium to
have charged exactly the cost, refunds the rest, and records the exact cost as
`setup_spent`. A caller can no longer over-fund the migration PDA or start a CPI
that Raydium would fail for lack of rent.

`open_time` may not be later than the current clock time (`OpenTimeInFuture`
otherwise), and the pool must come out `SwapOnly`. Without this bound any caller
could graduate a token with a far-future open time: Raydium would create the pool
waiting to open, nobody could swap, and the migration could not be redone because
the receipt and pool already exist. Pass `0` (or the current time) to open at once.

A migration uses about 300,000 compute units, around 136,000 of them in Raydium
itself, so a client must request a limit above the 200,000 default.

## What `migrate_amm_v4` does

1. Checks the curve is complete with no real token reserves, is not Mayhem, quotes in native SOL,
   and the mint is a classic SPL Token mint (a Token-2022 mint fails account validation).
2. Derives every Raydium account from the 19-account ABI and rejects any account whose key differs.
   The pool, LP mint, both vaults, target orders and the LP account must be uncreated.
3. Plans the amounts from authenticated Beam state before anything moves:
   all tokens in the curve vault, and `real_quote + post_complete_quote_in - pool_migration_fee`
   in SOL. Rent, creator and protocol fee claims and unrelated lamports on the curve are
   protected by the same floor the CPMM candidate used.
4. Moves `setup_budget` from the caller to the migration PDA, the planned tokens to its base
   account, and the planned SOL into its WSOL account, then `SyncNative`. Existing balances in
   those accounts are recorded first; donations are neither adopted nor swept.
5. Calls Raydium `Initialize2` with the migration PDA as wallet, signing with `invoke_signed`.
6. Re-reads the result and requires: pool owned by Raydium, 752 bytes, status `SwapOnly` or
   `WaitingTrade`; both vaults owned by Raydium's authority with the planned mints and amounts;
   the sources drained back to their prior balances; LP supply and the PDA's LP balance equal to
   `isqrt(coin * pc) - 10^decimals`; Raydium's fee destination up by exactly the create-pool fee.
7. Pays the inherited `pool_migration_fee` to `Global.withdraw_authority`, checks the curve still
   covers its protected floor, refunds the caller's unspent setup, writes the receipt, zeroes the
   curve's reserves and emits `BeamAmmV4Migration`.

An error at any step, including after the CPI, unwinds the whole transaction.

Lamports move directly (not by CPI) between the curve and other accounts. Solana rejects a CPI
while the caller's accounts are unbalanced, so the curve rides along on the `SyncNative` CPI
that follows its payout into the WSOL account, and the migration fee is paid only after the last
CPI. Keep that ordering when editing the handler. The `SyncNative` accounts are WSOL, then the Rent
sysvar, then the curve: the newer token program that Agave 4.x ships reads the second account as
Rent and fails with `InvalidArgument` on anything else. The program-test bank's older token program
ignores extras, so only the live validator exposed this.

### PDAs

| Account | Seeds (Beam program) |
| --- | --- |
| Config | `["beam-amm-v4-config"]` |
| Migration authority (system-owned signer) | `["amm-v4-migration-authority", mint]` |
| Receipt | `["beam-amm-v4-migration", mint]` |
| Rights record | `["beam-amm-rights", pool]` |

The seeds are versioned so state from the superseded CPMM adapter can never look valid here.
The receipt holds mint, pool, LP mint, amounts, both fees, LP issued, setup spent, `open_time`
and `lp_disposition`.

## LP tokens

The LP tokens stay in the migration PDA's associated token account and
`receipt.lp_disposition` is `0` (held, unresolved). Nothing burns, locks or transfers them, and
no Beam instruction can move them. That is not an LP custody design. It is the least committing
state while the maintainer decides between burn, lock and retain. Holding them keeps Beam's
fee-claim option open but also means that, until a release path exists, nobody can withdraw
principal. The options, with Raydium's fee mechanics from the pinned source, are in
[LP_CUSTODY_DECISION.md](LP_CUSTODY_DECISION.md); the choice is the owner's.

## Evidence

Beam is compiled to SBF and loaded next to the pinned Raydium Rust processor in one
`solana-program-test` 3.0.0 bank, so a single transaction runs **Beam handler -> real pinned
Raydium `Initialize2` -> SPL Token and associated-token programs -> Beam checks**. This is the
test PR #38 and #63 could not do separately. Beam cannot run as a Rust processor there because
Raydium at the pin needs `solana-program` 3.0 and Beam is on Anchor 0.31.1.

```sh
(cd programs/beam && cargo build-sbf --arch v0 --no-default-features --features no-idl,experimental-amm-v4)
(cd programs/beam && cargo build-sbf --arch v0 --no-default-features --features no-idl --sbf-out-dir target/deploy-production)
cd tests/amm-v4-validator
cargo test --locked --features beam-sbf --test beam_migration -- --test-threads=1
```

`--no-default-features` selects the mainnet AMM v4 program id, which is what the pinned crate
uses by default. `--arch v0` is required: `cargo-build-sbf` 4.4 defaults to SBPF v3, which the
3.0.0 test runtime rejects with `UnsupportedProgramId`. The `beam-sbf` feature keeps the existing validator CI job from needing an SBF
build.

Thirteen tests pass locally, and `.github/workflows/beam-amm-v4-sbf.yml` runs the same builds and
tests on a GitHub Linux runner (passed in a private fork). Cases covered: success with and without pre-existing source accounts and donations (exact vault,
LP, fee, curve, refund and receipt values, including the exact setup rent Raydium charged);
disabled config; replay; fee cap on both the caller and stored sides; setup funding one lamport
under the exact cost and one over the accepted worst case; an open time in the future refused
before anything moves; funding for a higher fee cap refunded
down to the exact cost; a production-style build (no activation feature) refusing to enable or run
migration; Token-2022 mint; pre-created pool; ten substituted accounts; and a later
instruction failing after a successful migration. Each failure compares every touched account byte
for byte against its state before the transaction. With the setup bounds, a caller can no longer
make Raydium fail mid-CPI for lack of rent, and Raydium's other checks all pass for state Beam
derived, so no test provokes an in-CPI Raydium failure; rollback after a completed CPI is covered.

The suite also passes against Raydium compiled to SBF (`RAYDIUM_SO`, run locally). The live
validator job in the same workflow builds Beam, builds Raydium at the pin, installs Agave
`v4.4.0` after checking its SHA-256, and migrates on `solana-test-validator`, then checks that a
replay fails and changes nothing.

`cargo check --locked --all-features` fails in `programs/beam` because of the inherited
`anchor-debug` feature (`solana_program` unresolved). That is independent of this work.

## AMM v4 quote path

`TASKS.md` and `STATUS.md` list an AMM v4 quote path as missing: a `create_v2` launch quoted in an
already-graduated token fails closed (PR #12) instead of reading an AMM v4 pool. This branch does not
add a second implementation. Draft PR #49 (stacked on #36, which conflicts with `main`) already has
`beam_amm_v4_quote.rs::graduated_record`, which values a graduated ancestor from raw AMM v4 pool and
vault bytes. This branch checks that adapter's assumptions against the pinned source and against
pools this instruction produces:

- **Layout.** `tests/pool_layout.rs` pins every hard-coded offset the #49 reader uses (status 0,
  pending PnL 192 and 200, vaults 336 and 368, mints 400 and 432, LP mint 464, market seed 528,
  target orders 592, size 752) to the packed `AmmInfo` at the Raydium pin. All match.
- **Preconditions.** After a real migration, `assert_quote_ready` checks what the #49 reader checks:
  `SwapOnly` status, zero pending PnL, identity fields equal to Beam-derived addresses (the mint is
  the market seed), vaults owned by Raydium's authority with no delegate or close authority, and a
  fully backed WSOL vault. A pool migrated here satisfies all of them.
- **Rights record, written by the migration.** #49's reader also requires an `AmmV4Rights` account
  for the pool. `migrate_amm_v4` now creates it (`BeamAmmV4Rights`, paid by the caller) with the same
  discriminator and bytes #49's `register` writes: pool, mint, market seed (the mint), the curve's
  creator as both creator and payout wallet, no donation selection, revision 0, not paused. A test
  compares the account byte for byte. When #49 lands, replace `BeamAmmV4Rights` with its
  `AmmV4Rights`; #49's `register` then fails for a migrated pool with account-already-in-use, which is
  intended. The layout follows #49 at head `32c661d` and must be re-checked if that PR changes.
- **Valuation.** The reader uses spot vault reserves minus pending PnL. That is not a TWAP or oracle
  and can be moved by one trade; it should not price anything an attacker can profit from
  without a manipulation bound.

## What this does not establish

- The tests run Raydium's source at the pin, both as the Rust processor and (with `RAYDIUM_SO`)
  as that source compiled to SBF, and Beam as an SBF build of this source. A live
  `solana-test-validator` run exists as the `live-validator` feature and a Linux CI job. That job
  is the first place it runs: it has not been executed on this machine, because Anza ships the
  validator for Linux only. None of this is the deployed mainnet Raydium binary, so
  deployed-program equivalence is still not established.
- Post-CPI pool checks read the pool status and vault and LP accounts, not the whole packed
  `AmmInfo`. Draft #34's source-pinned decoder is the better long-term check.
- The Beam IDL is not regenerated. The instructions are built from discriminators in the test.
- Token-2022 launches cannot graduate. AMM v4 rejects those mints; they need a separate plan.
- Raydium describes AMM v4 as legacy for new pools. The venue is the maintainer's selection and
  was not changed here; reaffirm it before activation.

## Open decisions and risks

1. **LP custody and fee rights.** See above.
2. **Fee funding.** The instruction keeps the inherited migration fee and recipient. How the
   42/5/3 allocation, creator funding and Raydium's create-pool fee relate is undecided.
3. **AMM seed is front-runnable.** The mint is the seed Raydium uses for the pool address, which
   keeps the pool deterministic. But `Initialize2` is permissionless and the seed is public:
   anyone holding a little of the token can create the pool for that seed first, at any price,
   and graduation then fails closed (tested) with no way to retry. A fresh caller-supplied seed
   recorded in the receipt avoids this, at the cost of a non-deterministic pool address. Pick one
   before activation.
4. **Rights record contents.** Creator payout defaults to the curve creator and donation selection
   starts empty, copied from #49's `register`. Those choices are that PR's proposal, not approved.
5. **Curve state.** Reserves are zeroed after migration, as the CPMM candidate did. A dedicated
   migrated flag on the curve would be clearer.
