# Raydium AMM v4 integration boundary

Beam selected Raydium AMM v4 for graduation. This page tracks the development
dependency chain. The current Beam migration instruction is a gated, superseded
CPMM candidate. A feature-gated, host-only Beam diagnostic now invokes the
AMM v4 `Initialize2` ABI against a synthetic local processor and always returns
an error, rolling back the transaction. **Production migration is not enabled.**

The ABI source pin is `raydium-io/raydium-amm@91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6`.
It is source evidence, not proof that a deployed Raydium program has matching
bytes. The archived Pump and CPMM snapshots remain provenance references.

| Step | Current local implementation | Still required before activation |
| --- | --- | --- |
| Account ABI | `programs/beam/src/beam_amm_v4.rs` derives the 19 `Initialize2` metas and validates actual classic SPL mint/source accounts, canonical Beam signer ATAs, WSOL backing, current config fee, and planned initial LP. | Fix and persist the market seed, store an authorized fee cap, preflight immediately before CPI, and check account aliases and all setup identities. Token-2022 graduation needs a separately reviewed compatible design. |
| Funding | `programs/beam/src/beam_amm_v4_funding.rs` computes checked curve liquidity, creator/protocol liabilities, donor surplus, migration charge, and caller-funded setup budget. The host-only funded probe reads these from current Global, curve, mint, vault, config and rent accounts, then moves base and SOL into canonical source ATAs before rolling back. | Pin launch parameters and migration fee policy: current Global reserve, fee and recipient fields can change. Verify the initial base entitlement independently of mutable Global. Do not use balance-only refund math without exact account and reserve checks. |
| Staged settlement | `programs/beam/src/beam_amm_v4_settlement.rs` checks three keyed observations. The funded probe uses its canonical-key/alias helper, the funding helper's setup-donor check, and its own keyed source, fee-recipient and fee-destination deltas. | Capture all three observations from authenticated AccountInfos in a real adapter. Bind every setup-rent payment to its created account; a total cap alone does not prove attribution. A caller-supplied `receipt_written` boolean is not replay protection. |
| Post-CPI snapshot | `programs/beam/src/beam_amm_v4_snapshot.rs` inspects actual AccountInfos against the pinned 752-byte packed `AmmInfo`, 2208-byte target-orders account, and classic SPL source/vault/LP/fee destination data. It checks the immediate status, account identities, token authorities, amounts, and WSOL backing. | Bind its market seed, amounts, time and program identity to authenticated Beam policy/Clock inputs; verify the real Raydium binary in an isolated local runtime. The raw layout has no Anchor discriminator and can change with program upgrades. |
| Runtime evidence | Host tests cover the source helpers. [PR #59](https://github.com/shawkkkkk/beam/pull/59) tests synthetic post-CPI account snapshots; `tests/runtime-harness/tests/amm_v4_cpi_probe.rs` exercises both Beam diagnostics, including funded source preparation, late CPI error, fee/liability checks, terminal rollback, and rejection of selected donations/aliases. Its AMM/SPL/ATA processors are mocks. [PR #38](https://github.com/shawkkkkk/beam/pull/38) separately tests the pinned Raydium source processor with a different Solana runtime version. | Test the complete migration against genuine classic SPL and pinned Raydium source processors in one isolated runtime, then validate a deployed binary separately. Resolve LP custody and fee rights. Never use captured original Pump binaries. |
| Client and indexing | AMM v4 ABI fixtures are source-only; the preview labels migration and liquidity unavailable. | Add a namespaced AMM v4 receipt/event, authenticated indexer mapping, SDK builder against the final account ABI, and verified-liquidity state. |

The planned transfer sequence is:

1. Verify the curve backing and separate fee/rent liabilities. Capture the
   initial balances and any donor surplus.
2. Move exactly the planned base amount from the curve's canonical base-token
   vault to Beam's base source and the planned pool quote from the curve to
   Beam's canonical WSOL source. Fund setup from the caller's bounded
   contribution. Capture the
   prepared balances. A WSOL source must be synced: lamports equal its native
   rent reserve plus token amount.
3. Invoke AMM v4 with the Beam PDA signer. Check the source debits, pool vault
   deposits, Raydium's canonical WSOL creation-fee destination, and received
   LP against the pinned integer-sqrt rule. Pay any existing migration fee only
   to its authorized recipient and verify the curve retains liabilities and
   donor surplus. Capture final balances and write an authenticated one-time
   receipt within the instruction.

The original probe starts with preseeded source ATAs. The funded probe exercises
curve-to-source transfers and a current-Global migration-fee recipient using
authenticated local-bank accounts. **Both always error and roll back**, even
after a valid synthetic pool snapshot. Neither decides LP custody or commits
a receipt. The SBF compile guard has not been tested with a Solana target. The
staged settlement helper's balance snapshots are still caller-supplied; the
funded probe does not call its three-stage `verify` with a fabricated receipt.
An executable adapter must bind
every value to the intended account, reject aliases before transfers, and
verify each destination's state. The setup-cost cap alone cannot prove rent
went only to canonical Raydium-created accounts. Incoming unrelated funds can mask overspend
in balance-only checks. A precreated pool or donated source tokens can block
migration and must not be adopted or swept to a caller.

The funded probe rejects a base vault unless its amount equals current
`token_total_supply - Global.initial_real_token_reserves - post_complete_base_out`.
This follows retained create/crossing-fill arithmetic, but the Global initial
reserve is mutable and not launch-pinned. The check is a conservative
diagnostic assumption, not production proof that the vault has no donated base.

AMM v4 does not natively accept Token-2022 mints or reproduce creator fees on
direct external swaps. LP disposition, post-graduation fee rights, creator
funding within or beyond the 42-bps community allocation, the internal
holder/buyback split, and production recipients remain undecided. Do not
enable migration or claim recurring buybacks from LP that Beam does not retain.

Host validation from the repository root:

```sh
python3 scripts/check_fork.py
cargo test --locked --manifest-path programs/beam/Cargo.toml
cargo test --locked --manifest-path programs/beam/Cargo.toml --features experimental-raydium
cargo check --locked --manifest-path programs/beam/Cargo.toml --no-default-features
# When the probe feature/test are present, with synthetic accounts and no live assets:
cargo test --locked --manifest-path tests/runtime-harness/Cargo.toml --features amm-v4-probe --test amm_v4_cpi_probe
```

These checks establish source integrity and deterministic host behavior only.
The probe tests exercise Beam diagnostic entrypoints and synthetic AMM, SPL
Token and associated-token processors; it does not exercise Beam's production
migration handler, the real SPL processors, the pinned Raydium source processor
in the same transaction, or a deployed Raydium binary. Synthetic cases cover
inner CPI errors, a post-CPI snapshot rejection, and valid packed snapshots
that reach the unconditional terminal error. Each case rolls back its listed
fixture accounts; transaction-payer fees are outside that assertion.
