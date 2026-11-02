# Compatible-source gate validation

Recorded 2 November 2026 with official Rust/Cargo 1.99.1 on x86_64 Linux.
This is native compatible-source execution with synthetic accounts and
valueless balances. It does not establish SBF or deployed-program behavior,
a committed Beam migration, Token-2022 graduation, or LP custody/fee policy.
Migration remains disabled and the host-only Solana-target guard is intact.

## Inputs

| Input | Exact revision |
| --- | --- |
| Earlier Beam baseline | `88255a9be80a66c35c716ba977654d82b8682284` |
| Publication parent, mint/freeze-authority guard | `27000bcf248873c91fab5ddeb7aaba168e2d00f5` |
| Raydium original Rust source | `91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6` |
| arrform | `fdbb0cfe18eadbff99cc8cd63f1b5a43df70e665` |

The [README](README.md#compatibility-adaptation) documents the separate
test-only manifest: Solana 3 to 2.2.1, system-interface 3 to 1.0.0,
SPL Token 9 to 7 and ATA 8 to 6. All 17 Raydium files, including LICENSE,
are exported unchanged and checked against original Git blobs and modes.
No captured program binary is loaded; native preference is explicitly set
before registering the Beam, Raydium, SPL and ATA processors.

## Observed results

| Check | Result |
| --- | --- |
| Baseline source gate, feature omitted | 1/1 pass: two inner initializations, terminal rollback, one late inner failure, nine pre-CPI refusals |
| Guard source gate, `mint-authority-guard` feature | 1/1 pass: same positive/late-failure paths and eleven pre-CPI refusals |
| Guard original synthetic runtime | 3/3 pass |
| Guard host default | 38 unit + 7 funding + 4 compatibility pass |
| Guard host `experimental-raydium` | 38 unit + 7 funding + 4 compatibility pass |
| Guard host `amm-v4-probe` | 38 unit + 7 funding + 4 compatibility pass |
| Guard `--no-default-features` check | Pass |
| Baseline and guard provenance | 380 archived Git blobs each; 62 retained files equal and four existing declared edits |

The two inner successes are followed by Beam error 7002 and full tracked-value
rollback. The second uses a new blockhash; it shows failed-diagnostic
retryability. A separate underfunded setup case reaches a real System error
after Raydium's fee transfer and target/mint/vault allocations, then restores
all tracked value accounts. Transaction-payer fees are excluded from these
snapshots. The authority-negative controls were also checked for sensitivity
against the earlier baseline; the first active-mint control failed as expected.

An independent local reviewer inspected the source, dependency adaptation and
completed logs, verified the 17 original blobs, and found no remaining must-fix
in this bounded gate. This is peer analysis, not a formal GitHub approval or
security audit. The publication commit reruns the guarded source gate and
provenance; its logs remain with the local evidence.

## Reproduce

Use the source preparation and guarded runner commands in
[README](README.md#run). Keep a different `CARGO_TARGET_DIR` for each standalone
manifest/lockfile. After source preparation and `cargo fetch --locked`, the
runner uses `--locked --offline`, checks source before and after execution,
and records the actual Git head. Use another target for the original harness:

```sh
CARGO_TARGET_DIR=/workspace/beam-runtime-target cargo test --locked --manifest-path tests/runtime-harness/Cargo.toml --features amm-v4-probe --test amm_v4_cpi_probe --jobs 3 -- --test-threads=1
python3 scripts/check_fork.py
git show --format=fuller --stat HEAD
git show --format= --check HEAD
```

## Retained local evidence

Full logs, source receipts, original licensed source, tool versions and build
outputs remain outside Git. The pre-publication evidence bundle SHA-256 is
`48a8d0705a6b0d73078181eccfea39e9fe41769af94e1aebd89e308e2510ea88`.
Selected hashes allow that evidence to be checked without committing logs:

| Evidence | SHA-256 |
| --- | --- |
| Baseline source-gate log | `b79f9391fa4822741c35566c53db9b341c5515bc612fee215774676a0f7698c9` |
| Guard source-gate log | `b499de92fe77c112aa41eb0afbe671c92acb28c40c732682f6d4ae1c5c14f3b1` |
| Independent review receipt | `3885af5be8ee5b8895a0bd6f34be41dd25cc56d97690392d5eea70240c04ae42` |
| Gate Cargo.lock | `6d7b417d68b5e32b555daac852d08343621dc9253167585a7d7c7a2b67163fe9` |

The pre-publication review covered the eight gate files and parent guard;
publication adds this record, clarifies the README reproduction command and
normalizes trailing manifest whitespace.
No deployment, RPC, wallet/program keypair files, secrets, permissions,
economic policy or runtime activation is part of the gate.
