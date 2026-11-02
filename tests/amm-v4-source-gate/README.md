# Beam funded diagnostic with compatible Raydium source

This isolated, local-only gate invokes Beam's actual
`ProbeAmmV4FundedInitialize2` entrypoint and Raydium's actual
`Processor::process` in one Solana 2.2.1 ProgramTest bank. Beam still returns
`MigrationNotEnabled` after a successful inner initialization. No migration
is activated and no transaction commits the pool or reserve movement.

Beam baseline: `88255a9be80a66c35c716ba977654d82b8682284`.
This gate's publication parent is the mint/freeze-authority guard at
`27000bcf248873c91fab5ddeb7aaba168e2d00f5`, directly atop that baseline.
Raydium source: `raydium-io/raydium-amm` at
`91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6`.
The full 17-file source export, including its Apache-2.0 license, is verified
by exact Git path, mode and blob. It is generated under ignored `.source/`;
neither Beam's immutable archive nor Raydium's source files are edited.

## Compatibility adaptation

The separate `raydium-compat/Cargo.toml` points directly at the verified
original Rust source. It substitutes these dependencies to use Beam's native
bank and ABI. The upstream manifest stays unchanged in the export.

| Dependency | Original Raydium manifest | Local compatibility manifest |
| --- | --- | --- |
| solana-program | =3.0.0 | =2.2.1 |
| solana-system-interface | =3.0.0 | =1.0.0 |
| spl-token | 9.0.0 | =7.0.0 |
| spl-associated-token-account | 8.0.0 | =6.0.0 |
| arrform | unpinned Git dependency | fdbb0cfe18eadbff99cc8cd63f1b5a43df70e665 |

Other dependency requirements are retained and resolved by this gate's own
lockfile. The upstream dev dependencies and release profiles are omitted;
this gate builds native test code. The default `devnet` test feature selects
both Beam's devnet identity and Raydium's devnet source constants; explicit
`mainnet` selects the corresponding mainnet identities in this local bank.
Both or neither test profiles are rejected at compile time. The test uses
native SPL Token 7 and ATA 6 processors. `prefer_bpf(false)` is set
**before** adding any processor so
ambient `BPF_OUT_DIR`/`SBF_OUT_DIR` files cannot replace these source entries.

This is **compatible pinned-source validation**, with synthetic accounts and
valueless balances. It is not an unmodified Solana 3 dependency build, an SBF
test, deployed-bytecode validation, a committed migration, or Token-2022
graduation evidence. LP custody, principal/fee rights, recipient policy and
mutable Global entitlement limitations remain unresolved.

The opt-in `sbf-raydium` backend separately loads the newly compiled original
Raydium ELF with its original dependency graph. It keeps Beam's diagnostic
and SPL/ATA native. See the [SBF gate instructions](../sbf-support/README.md)
for the compiled default Beam gate, original Raydium SBF profiles and the
different evidence levels. The default native backend above is unchanged.

## Run

Use a separately approved official Rust toolchain (verified here with
Rust/Cargo 1.99.1). This gate installs no tools, creates no wallet/program
keypair files, contacts no RPC and submits no live transactions. ProgramTest
and its fixture use ephemeral in-memory test signers for local bank messages.

From the repository root, obtain the public source without running hooks:

```sh
git clone --no-checkout https://github.com/raydium-io/raydium-amm /workspace/raydium-gate-source
git -C /workspace/raydium-gate-source checkout --detach 91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6
python3 tests/amm-v4-source-gate/prepare_source.py --repository /workspace/raydium-gate-source
export CARGO_TARGET_DIR=/workspace/beam-gate-target
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo fetch --locked --manifest-path tests/amm-v4-source-gate/Cargo.toml
bash tests/amm-v4-source-gate/run_gate.sh /workspace/raydium-gate-source --features mint-authority-guard
```

Use an isolated `CARGO_HOME` too when sharing a machine with another build.
Use a different target directory for each standalone manifest/lockfile. Beam
builds both `cdylib` and `lib`; sharing its outputs with the host or synthetic
harness can select an incompatible `libbeam_program.rlib` from their different
dependency graphs. Do not reuse this gate's target directory for those checks.
Preparation rejects missing, altered or extra source files; the runner checks
the export both before and after the locked, offline test. Preserve stdout,
stderr, the lockfile and `.source/receipt.json` with any result.

The funded fixture derives from the reviewed synthetic test at the Beam
baseline. The gate substitutes actual source processors; it does not include
the synthetic AMM, SPL or ATA implementations. One sequential test covers:

- Successful inner initialization: pool/target-order sizes and owner, both
  vault amounts, LP amount, Beam post-CPI snapshot and terminal error 7002.
- A fresh-blockhash retry after rollback, reaching the source again. This is
  retryability of the failed diagnostic, not replay rejection after migration.
- A real inner System-program failure for insufficient pool setup rent, after
  Raydium has charged its fee and allocated target/mint/vault accounts; every
  tracked value account returns to its before state when the transaction fails.
- Nine refusals before Raydium: precreated pool, source token/WSOL donations,
  recipient aliases, donated curve base, insufficient quote backing and quote
  overflow, and insufficient initial LP. Curve rent, creator/protocol
  liabilities, donor surplus, setup donor balance, recipients and pool/source
  accounts are part of the complete rollback snapshots. The separate
  transaction fee payer is excluded.

The additional `mint-authority-guard` test feature adds two source-gate
refusals for remaining base mint/freeze authorities. Use it when checking
guard head `27000bcf248873c91fab5ddeb7aaba168e2d00f5`, directly atop the
baseline; the reproduction command above includes it. Omit the feature only
when reproducing the earlier baseline's nine-refusal gate. The runner records
the actual Beam Git head in its output. These two controls are expected to fail
against the earlier baseline and are not claimed as baseline protection.

## Local identity matrix

After the same source preparation and locked dependency fetch, run both
profiles with separate build outputs:

```sh
CARGO_TARGET_DIR=/workspace/beam-identity-matrix bash tests/amm-v4-source-gate/run_identity_matrix.sh /workspace/raydium-gate-source
```

The runner uses `<target>/devnet` and `<target>/mainnet`. Its explicit features
are `--no-default-features --features <profile>,mint-authority-guard`; the
profile configures both Beam and the pinned Raydium source. This compiles
native local tests using public mainnet/devnet addresses as fixture keys.
It does not connect to either network or change a deployed program.

Each profile repeats the two successful inner initializations with terminal
rollback, one late inner failure and eleven input refusals. Four additional
cases substitute the opposite-profile executable program, config address,
config owner or fee destination. They assert the specific Beam/Anchor error,
no new Raydium call, zero calls to an opposite-profile native trap and unchanged
complete account snapshots. That trap only detects unexpected dispatch; it is
not an AMM implementation. Every referenced account and both profiles'
canonical config/fee accounts are snapshotted, including absent accounts;
the distinct transaction fee payer remains excluded.

The [validation record](VALIDATION.md) distinguishes the baseline and guard
results and records the retained local evidence hashes.

## SBF tooling boundary

No `cargo build-sbf` was invoked. The published official
`solana-cargo-build-sbf` 4.0.0 crate was inspected as source only (crate
SHA-256 `2721d1d4d623e43f0f0193b07c60455b45b039cb392931bed5c03af39874a257`).
Its `src/post_processing.rs` calls `generate_keypair` when a program keypair
is absent in either release or debug mode. No documented compile-only switch
was established in this inspection. This gate uses the compatible native
source approach by default; it does not patch the builder or generate a
program keypair. Later source-built SBF tests use direct Cargo with the
specifically approved official toolchain, as documented separately. Beam's
`amm-v4-probe` Solana-target compile guard remains intact.
