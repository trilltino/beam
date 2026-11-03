# Gated Raydium AMM v4 migration instruction

`migrate_amm_v4` in [`beam_amm_v4_migration.rs`](../programs/beam/src/beam_amm_v4_migration.rs)
is Beam's first executable AMM v4 graduation path. It is **disabled in every
production build**. It exists so the Beam-to-Raydium path can be exercised
against the pinned Raydium processor while the product decisions below stay open.

This page covers one bounded lane. LP issued to Beam is burned after verification;
the creator payout rate and Token-2022 plan remain open. It does not touch `upstream/`. Draft PRs #29, #30, #33,
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

Before the LP burn was added, a migration measured 208,623 compute units on a validator
(see "Measured cost"). The burn increases that cost; a client must request a measured,
reviewed limit above the 200,000 default.

## What `migrate_amm_v4` does

1. **Preflight** (nothing moves). Checks the curve is complete with no real token reserves, is not
   Mayhem, quotes in native SOL, the mint is a classic SPL Token mint (a Token-2022 mint fails account
   validation) with at most 18 decimals, and `open_time` is not in the future.
2. Derives every Raydium account from the 19-account ABI and rejects any account whose key differs.
   The pool, LP mint, both vaults, target orders and the LP account must be uncreated.
3. Plans the amounts from authenticated Beam state:
   all tokens in the curve vault, and `real_quote + post_complete_quote_in - pool_migration_fee`
   in SOL. Rent, creator and protocol fee claims and unrelated lamports on the curve are
   protected by the same floor the CPMM candidate used. Bounds the setup budget (below).
4. **Baseline.** Requires the migration PDA's base and WSOL accounts to be initialized, unfrozen,
   delegate-free and close-authority-free, and records their balances: donations are neither adopted
   nor swept.
5. **Funding.** Moves `setup_budget` from the caller to the migration PDA, the planned tokens to its
   base account, and the planned SOL into its WSOL account, then `SyncNative`.
6. Calls Raydium `Initialize2` with the migration PDA as wallet, signing with `invoke_signed`. The 18
   accounts other than the seed must be distinct keys.
7. **Verification.** Decodes the new pool field by field (see "Pool verification and source policy"),
   and requires both vaults to hold the planned amounts under Raydium's authority, the sources to be
   back at their baselines, the LP supply and the PDA's LP balance to equal
   `isqrt(coin * pc) - 10^decimals`, Raydium's fee destination up by exactly the create-pool fee, and
   Raydium to have charged exactly the planned setup cost.
8. Burns exactly the LP issued to the migration PDA, then verifies the mint supply and PDA
   LP balance are zero. A failed burn rolls back pool creation and funding.
9. Refunds the caller's unspent setup, pays the inherited `pool_migration_fee` to
   `Global.withdraw_authority`, checks the curve still covers its protected floor, writes the rights
   record and the receipt, zeroes the curve's reserves and emits `BeamAmmV4Migration`.

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

The maintainer chose the pre-PumpSwap Pump.fun pattern: burn the LP minted to Beam
at graduation. The migration PDA receives the LP from Raydium, Beam verifies the exact
amount, then burns it in the same transaction. `receipt.lp_amount` records the issued
amount and `receipt.lp_disposition` is `1` (burned). The PDA's LP account and the LP
mint supply are zero after the burn. Raydium's unminted minimum LP remains reflected
in its pool accounting. Initial liquidity cannot be withdrawn through the burned
position, and Beam has no ongoing claim to that position's AMM v4 LP fees. Creator
payouts, community distributions and buybacks after graduation therefore need a
separately approved, actually collected source. See [the decision record](LP_CUSTODY_DECISION.md).

## Evidence

These are different claims and are not interchangeable. Each row says what was run and what it
does **not** show.

| Level | Result | Where | Does not show |
| --- | --- | --- | --- |
| Raydium source correctness | Pool layout offsets, fee constants and status rules read from the pinned source and pinned by tests (`pool_layout.rs`, 3 tests) | local, CI | That any deployed binary matches that source |
| Rust processor integration | 19 migration tests passed before the LP burn; rerun on this head | prior PR #68 local, CI | The burn or compiled bytecode |
| Compiled-SBF integration | The same 19 passed before the LP burn with the pinned source compiled to SBF (`RAYDIUM_SO`); rerun on this head | prior PR #68 local, CI | The burn or deployed binary |
| Local validator | One migration and failed replay passed before the LP burn; rerun on this head | prior PR #68 CI (Linux only) | The burn or mainnet state |
| Deployed-program identity | `675kPX9M…` is executable under the upgradeable loader; program-data `A7ZG7By…`; last deployed slot 445763204; upgrade authority `FytDrVz…`, a single key, so Raydium can change the binary at any time (read-only RPC, 3 Nov 2026) | local script, CI dump | That the binary is unchanged since |
| Deployed-binary behavior | Before the LP burn, the binary dumped from mainnet completed the same live migration on the validator (208,441 compute units); rerun on this head | prior PR #68 CI, informational step | The burn or real mainnet accounts: Raydium config and fixtures are synthetic |
| Deployed-binary equivalence | **Not established.** The deployed ELF (308,601 bytes, SHA-256 `d4cee0cd…7b2c9f` with trailing padding trimmed) differs from the pinned build (288,752 bytes). No verified-build attestation was found or checked | read-only RPC | n/a |
| Mainnet compatibility | **Not tested.** No mainnet transaction, funds or wallet were used | | |

`solana-program-test` 3.0.0 rejects the mainnet binary before running any of it ("Program is not
deployed"), even under the upgradeable loader. That is a limit of that test bank, not evidence of a
behavioral difference: the 4.4.0 validator runs the same bytes.

```sh
(cd programs/beam && cargo build-sbf --arch v0 --no-default-features --features no-idl,experimental-amm-v4)
(cd programs/beam && cargo build-sbf --arch v0 --no-default-features --features no-idl --sbf-out-dir target/deploy-production)
cd tests/amm-v4-validator
cargo test --locked --features beam-sbf --test beam_migration -- --test-threads=1
# the same suite against compiled Raydium:
RAYDIUM_SO=/path/to/raydium_amm.so cargo test --locked --features beam-sbf --test beam_migration -- --test-threads=1
```

`--no-default-features` selects the mainnet AMM v4 program id, which is what the pinned crate
uses by default. `--arch v0` is required: `cargo-build-sbf` 4.4 defaults to SBPF v3, which the
3.0.0 test runtime rejects with `UnsupportedProgramId`. The `beam-sbf` feature keeps the existing
validator CI job from needing an SBF build.

### What the 19 migration tests cover

Success with and without pre-existing source accounts and donations (exact vault, LP, fee, curve,
refund, receipt and rights values, and every field of the new pool); disabled config; replay; the fee
cap on both the caller and stored sides; setup funding one lamport under the exact cost and one over
the accepted worst case; funding for a higher fee cap refunded to the exact cost; an open time in the
future; a mint with 19 or more decimals; frozen base and WSOL sources; an incomplete curve; a curve one
lamport short of its rent, fee claims and quote (refused) and one that exactly covers them (migrates and
leaves exactly its floor); an empty and an oversized curve vault; a production-style build that
cannot enable migration; a Token-2022 mint; a pre-created pool; a real third-party `Initialize2`
followed by a fail-closed migration; twelve substituted accounts (including the receipt and rights record); and a later instruction failing after a
successful migration. Each failure compares every touched account byte for byte with its state before
the transaction.

One case runs only on the live validator: a migration given 100,000 compute units fails and changes
nothing. `solana-program-test` applies its own global limit and ignores a transaction's
`SetComputeUnitLimit`, so the check means nothing there.

### Atomicity: where a failure can happen

| Failure point | Reachable? | Evidence |
| --- | --- | --- |
| Before funding | Yes | Every refusal above leaves all 17 watched accounts byte-identical |
| After partial funding | No | The causes (frozen or delegated sources, wrong balances, bad budget, bad accounts) are rejected in `preflight` and `baseline`, before any lamport or token moves |
| During the Raydium CPI | No | With the bounds above no input makes Raydium fail; a pre-created pool is refused before funding |
| After a successful CPI | Yes | `failure_after_a_successful_migration_rolls_back_the_pool` appends a failing instruction and checks the pool, receipt and every balance are gone |
| During final settlement | No | The fee payout is bounded by the same floor `preflight` already proved |
| Receipt creation | Yes | A replay fails at account validation with the pool and receipt untouched |

Solana makes the whole transaction atomic, so the unreachable rows rest on that guarantee plus the
checks that remove their causes, not on a test that provokes them.

## Measured cost

Measured on PR #68 before the LP burn, on `solana-test-validator` 4.4.0 by simulating
the transaction (a measurement, not an estimate), unless marked. **These compute figures
are stale for this head and must be refreshed from the hosted validator run.**

| Quantity | Value |
| --- | --- |
| Compute units, whole transaction | 208,623 (default limit is 200,000, so a client must request more) |
| Compute units, `solana-program-test` 3.0.0 bank | 299,763 (Raydium 135,823; associated-token program 20,389); the difference from the validator figure is not explained |
| Raydium on the deployed binary | 208,441 total |
| Transaction size | 1,100 bytes (limit 1,232) |
| Instruction accounts | 27, plus the compute-budget program |
| Raydium setup cost, exact (computed) | 54,962,800 lamports at a 25,000,000 create-pool fee; 29,962,800 of it rent |
| Extra Beam rent, paid by the caller (computed) | receipt 2,011,440; rights record 2,345,520 |

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
  for the pool. `migrate_amm_v4` creates it (`BeamAmmV4Rights`, paid by the caller) with the same
  discriminator and bytes #49's `register` writes: pool, mint, market seed (the mint), the curve's
  creator as both creator and payout wallet, no donation selection, revision 0, not paused. A test
  compares the account byte for byte. When #49 lands, replace `BeamAmmV4Rights` with its
  `AmmV4Rights`; #49's `register` then fails for a migrated pool with account-already-in-use, which is
  intended. The layout follows #49 at head `32c661d` and must be re-checked if that PR changes.
- **Valuation.** The reader uses spot vault reserves minus pending PnL. That is not a TWAP or oracle
  and can be moved by one trade; see [AMM_V4_RECONCILIATION.md](AMM_V4_RECONCILIATION.md).

## Pool verification and source policy

`verify_pool` decodes the new pool with `beam_amm_v4_pool_state.rs` and compares status, authority
bump, both decimals, the eight fee words, pending PnL, open time, both vaults, both mints, the LP
mint, the pool seed, the target-orders address, the absence of any OpenBook link, and the LP total
(the user's LP plus the permanently locked `10^coin_decimals`). It then checks the target-orders
account's owner and length. Reads are bounds-checked slices; there is no pointer cast and no
indexing that can panic.

Draft #29 and #34 check source and post-CPI balances for **exact equality**. That is stricter than
this branch on purpose does not follow: anyone could donate one token unit to the migration PDA's
token account and make every migration fail forever. Here donations are recorded in `baseline` and
preserved, while the sources must still be initialized, unfrozen, delegate-free and
close-authority-free (`InvalidMigrationSource`). #29 also found that Raydium's initializer overflows
a `u64` for a mint with 19 or more decimals; that guard is adopted (`MAX_SAFE_MINT_DECIMALS`).
#34's alias rule would reject the mint-as-seed design, because the seed and the coin mint are the
same key; the check here exempts only that one position.

## Pool-seed pre-emption

The pool address is derived from the launch mint, which is public. Raydium's `Initialize2` is
permissionless and treats the "market" account only as a PDA seed, so a third party can create the
pool first. The test `a_third_party_can_create_the_pool_first_and_beam_then_fails_closed` does exactly
that through the real processor, and Beam then refuses to migrate and moves nothing.

- **What the attacker needs:** a little of the launch token (any amount whose pool would issue more
  than the locked `10^decimals` LP), some SOL, Raydium's create-pool fee and the rent of the six
  accounts (about 0.03 SOL in the fixture). They receive the LP, so the liquidity is theirs to
  withdraw later; their net cost is mostly the fee.
- **Effect:** availability, not theft. The curve keeps its tokens and SOL, but this instruction can
  never graduate it, and the curve is complete so holders cannot sell there. There is no retry.
- **Alternatives the pinned processor supports:** any public key works as the seed.
  1. *A caller-supplied fresh seed*, recorded in the receipt and rights record (the rights record
     already has a `market_seed` field and the #49 reader uses it). A retry after a pre-emption then
     works. It is **not** front-running resistant: the seed is visible in the pending transaction, so
     an attacker can copy it and land first, but each such attempt costs them the fee and rent and
     the caller simply retries with a new seed, ideally through a private submission path.
  2. *A reservation step on Beam's side* does not help: Raydium never consults Beam, so a Beam-side
     reservation cannot stop a direct `Initialize2`.
  3. *Adopting an existing pool* (depositing into the attacker's pool) is unsafe, because the attacker
     chose its price.
- **Status:** not implemented. Choosing a seed policy changes how a pool is discovered (from the
  mint, or from the receipt), so it is a protocol decision. Until it is made the safe failure above
  stands and the availability risk is accepted and documented.

## Raydium CPI crate decision

Beam keeps its own source-pinned adapter (`beam_amm_v4.rs`) and does not depend on
`raydium-io/raydium-cpi`. `master` of that repository pins Anchor 1.0.2. Its `anchor-0.31.1` branch
matches Beam's Anchor version, but its `Initialize2` context is the older 21-account form with
OpenBook placeholder accounts (`amm_open_orders`, `market_program`), not the 19-account form Beam sends
and the pinned processor accepts. Beam would still have to derive and validate every address and
sign for the migration PDA itself, so the crate would add a git dependency and OpenBook accounts for no
correctness gain. Revisit if that crate gains a 19-account `Initialize2`.

## What this does not establish

- Deployed-binary equivalence and mainnet compatibility; see the evidence table.
- The `solana-program-test` figure for compute units differs from the validator's and is unexplained.
- Token-2022 launches cannot graduate. AMM v4 rejects those mints; they need a separate plan.
- Raydium describes AMM v4 as legacy for new pools. The venue is the maintainer's selection and
  was not changed here; reaffirm it before activation.
- `cargo check --locked --all-features` fails in `programs/beam` because of the inherited
  `anchor-debug` feature; that is independent of this work.

## Open decisions and risks

1. **No post-graduation LP fee claim.** The burned position cannot fund recurring payouts;
   define another source before promising any. See [LP_CUSTODY_DECISION.md](LP_CUSTODY_DECISION.md).
2. **Fee routing.** The owner chose creator payouts from the 42-bps community share.
   The creator rate and its remaining community distribution are unset. The instruction
   keeps the inherited migration fee and recipient; reconcile that fee and Raydium's
   create-pool fee before activation.
3. **Pool-seed policy.** See "Pool-seed pre-emption". Pick one before activation.
4. **Rights record contents.** Creator payout defaults to the curve creator and donation selection
   starts empty, copied from #49's `register`. Those choices are that PR's proposal, not approved.
5. **Curve state.** Reserves are zeroed after migration, as the CPMM candidate did. A dedicated
   migrated flag on the curve would be clearer.
