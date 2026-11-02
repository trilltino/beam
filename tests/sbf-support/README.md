# Local source-built SBF gates

These opt-in tests load explicit, SHA-256-verified ELF bytes into valueless
ProgramTest banks. They perform no RPC, deployment or external transfer.
The framework uses ephemeral in-memory test signers; no wallet/program-keypair
files, credentials, rustup links or security settings are created.

| Gate | Executed programs | Evidence boundary |
| --- | --- | --- |
| `beam-sbf` | Unmodified default Beam source compiled for SBF, devnet identity | Retired adapter disable/authority/rollback cases; default ELF rejects the absent funded diagnostic with Anchor 101 |
| `sbf-raydium`, devnet | Native Beam diagnostic, locally compiled pinned Raydium SBF, native SPL/ATA | Actual cross-program execution and complete rollback in one local bank |
| `sbf-raydium`, mainnet | Same arrangement with matched mainnet identities | Same local checks with public mainnet addresses used only as fixture keys |

There is **no compiled Beam migration CPI test** here. The production
`amm-v4-probe` Solana-target compile guard remains intact, and an expected
compile failure verifies it. No guard-relaxed SBF test variant is created.
These are source-built local artifacts, not identified deployed/production
binaries. LP custody, fee rights and Token-2022 graduation remain unresolved.

## Tool and source pins

- Beam program and lockfile are unchanged from
  `960895c4fa796a1353e1d3454dff95e2e9552daa`; this follow-up changes test code.
- Original Raydium: `91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6`, including its
  original Solana 3 / SPL Token 9 / ATA 8 dependencies and original lockfile.
  Unlike the native compatibility gate, its executed SBF artifact has no
  dependency substitutions. All 17 source blobs and LICENSE are preserved.
- Official Anza platform-tools `v1.57`, Linux x86_64 archive:
  [official asset](https://github.com/anza-xyz/platform-tools/releases/download/v1.57/platform-tools-linux-x86_64.tar.bz2),
  531380716 bytes, SHA-256
  `b0f7af104adf726fff2a6a09ea2eb2f2d2965c92295f4d7388c08d140e0c2b00`.
  Observed rustc `1.95.0-dev`, commit
  `ae660768afd467d131776918a1627e18920b230c`; Cargo `1.95.0`
  (`bbbd3a760`); Clang/LLVM `22.1.2`.
- Target `sbpf-solana-solana`, SBPF v0. Direct Cargo compilation avoids
  `cargo-build-sbf` post-processing, which generates missing program keypairs.
  The [official Agave changelog](https://github.com/anza-xyz/agave/blob/9ebddda13db3e63cc107e5fc7a3939e45891d943/CHANGELOG.md#L581-L583)
  documents direct Cargo use. Its `--triple` example is a documentation typo;
  Cargo and the official builder use `--target`.

Do not substitute captured binaries or arbitrary downloaded tools. Installation
and execution of an additional toolchain need the applicable authorization.
No Raydium program binary was downloaded or captured. Every Raydium ELF used
here was compiled locally with the approved toolchain from the source pin and
original lockfile above. For this SBF phase, that approved Anza archive was
the only newly downloaded executable bundle. The host Rust 1.99.1 toolchain
was already installed from a verified official Rust archive in the earlier
native-gate phase.

The commands below assume this exact official archive was already verified
and extracted into the approved isolated directory; no installer runs.

## Compile only

From the Beam repository root, with a clean pinned Raydium checkout and the
verified toolchain already present, use separate output directories:

```sh
(
  task_sbf_root=/workspace/beam-build-gate/sbf-tools/v1.57
  task_ray_checkout=/workspace/beam-build-gate/sources/raydium-amm
  export PATH="$task_sbf_root/rust/bin:$task_sbf_root/llvm/bin:$PATH"
  export RUSTC="$task_sbf_root/rust/bin/rustc"
  export CC="$task_sbf_root/llvm/bin/clang" AR="$task_sbf_root/llvm/bin/llvm-ar"
  export OBJDUMP="$task_sbf_root/llvm/bin/llvm-objdump" OBJCOPY="$task_sbf_root/llvm/bin/llvm-objcopy"
  export CARGO_HOME=/workspace/beam-build-gate/cargo
  git -C "$task_ray_checkout" diff --exit-code
  cargo fetch --locked --target sbpf-solana-solana --manifest-path "$task_ray_checkout/program/Cargo.toml"
  cargo fetch --locked --target sbpf-solana-solana --manifest-path programs/beam/Cargo.toml
  CARGO_TARGET_DIR=/workspace/beam-build-gate/raydium-sbf-mainnet-target cargo build --locked --offline --release --target sbpf-solana-solana --manifest-path "$task_ray_checkout/program/Cargo.toml" --jobs 3
  CARGO_TARGET_DIR=/workspace/beam-build-gate/raydium-sbf-devnet-target cargo build --locked --offline --release --target sbpf-solana-solana --manifest-path "$task_ray_checkout/program/Cargo.toml" --features devnet --jobs 3
  CARGO_TARGET_DIR=/workspace/beam-build-gate/beam-sbf-devnet-target cargo build --locked --offline --release --target sbpf-solana-solana --manifest-path programs/beam/Cargo.toml --jobs 3
)
```

Check that the source checkout HEAD is the exact Raydium pin above, and verify
source/provenance again after building. The `.so` files are under each target's
`sbpf-solana-solana/release/`, named `raydium_amm.so` and `beam_program.so`.
No `cargo build-sbf`, Solana CLI, deployment command or keypair generator runs.
Any feature-enabled diagnostic SBF build must still fail at its existing
compile guard; do not remove it to obtain a binary.

## Observed local artifacts

These are the unstripped direct-Cargo outputs of the commands above. They are
local test artifacts and have not been compared with deployed bytecode.

| Artifact | Source/profile | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Beam | Unchanged program tree from `960895c...`, default `raydium-devnet`; diagnostic feature absent | 1750784 | `0b851a16e7d51ec9e13ee060d589a02ca2d4614c206dcbd39965b4e888de8da0` |
| Raydium mainnet | Pin `91739d8...`, original manifest/lock, default `program` feature | 344568 | `69459fd579f30aeaacd7b2c27be9eed633193878018f75c67938ffda8e1e0293` |
| Raydium devnet | Same pin/manifest/lock, `program` + `devnet` features | 344568 | `fef0a71418db223eaad51f32c2dc330f0215dbdb8d674871907024b77d7b26eb` |

All use the verified toolchain and SBPF v0 target above. Runtime is
`solana-program-test = 2.2.1`, running on host Rust/Cargo 1.99.1. Mixed-bank
Beam is native with `amm-v4-probe` and the selected identity; SPL Token 7 and
ATA 6 are native. The actual Raydium VM code uses its original Solana 3 /
SPL Token 9 / ATA 8 build graph. No native Raydium processor is registered in
that backend. Compiled default Beam is tested in a separate context under
its fixed ID, without diagnostic functions or a guard exception.

## Run the local gates

Use the existing approved host Rust/Cargo 1.99.1. Export six artifact variables:
`BEAM_SBF_ELF_PATH` / `BEAM_SBF_ELF_SHA256`,
`RAYDIUM_SBF_DEVNET_ELF_PATH` / `RAYDIUM_SBF_DEVNET_ELF_SHA256`, and
`RAYDIUM_SBF_MAINNET_ELF_PATH` / `RAYDIUM_SBF_MAINNET_ELF_SHA256`.
Take each path and SHA-256 from the verified local compilation receipt.
The loader fails before execution on missing inputs or a wrong hash and has
no ambient `.so` lookup or native Raydium fallback. Then run:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_TARGET_DIR=/workspace/beam-sbf-host-tests bash tests/sbf-support/run_sbf_gate.sh
python3 scripts/check_fork.py
```

The runner separates the runtime harness and both identity profiles into
different target directories. It records the exact host source head and runs
locked/offline. Keep full logs, ELF/tool/source hashes and original licenses
outside Git. Native-source comparison remains available through
`tests/amm-v4-source-gate/run_identity_matrix.sh`.

For each original-Ray SBF profile, transaction metadata must show the exact
Raydium invocation, success and SBF compute metering before terminal Beam 7002.
The failed setup case shows four completed token CPIs and the final failing
System funding call, followed by full transaction rollback. ProgramTest can
repeat adjacent lifecycle records, so those adjacent duplicates are normalized
before counting. This is evidence of inner execution flow; intermediate SBF
account bytes, fee deltas and reserve deltas are not directly observed as they
are by the native Raydium wrapper. Complete before/after snapshots and all
input/identity refusal cases remain required.
