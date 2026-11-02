#!/usr/bin/env bash
set -eu
support_root=$(cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(cd -- "$support_root/../.." && pwd)
: "${CARGO_TARGET_DIR:?Set an isolated host-test output directory}"
: "${BEAM_SBF_ELF_PATH:?Supply the locally built default Beam ELF}"
: "${BEAM_SBF_ELF_SHA256:?Supply its verified SHA-256}"
: "${RAYDIUM_SBF_MAINNET_ELF_PATH:?Supply the original pinned mainnet ELF}"
: "${RAYDIUM_SBF_MAINNET_ELF_SHA256:?Supply its verified SHA-256}"
: "${RAYDIUM_SBF_DEVNET_ELF_PATH:?Supply the original pinned devnet ELF}"
: "${RAYDIUM_SBF_DEVNET_ELF_SHA256:?Supply its verified SHA-256}"
host_target_root=$CARGO_TARGET_DIR
BEAM_SOURCE_HEAD=$(git -C "$repository_root" rev-parse HEAD)
export BEAM_SOURCE_HEAD
printf 'SBF gate host source head: %s\n' "$BEAM_SOURCE_HEAD"
CARGO_TARGET_DIR="$host_target_root/beam" cargo test --locked --offline \
  --manifest-path "$repository_root/tests/runtime-harness/Cargo.toml" \
  --test migration_runtime --features beam-sbf --jobs 3 -- --test-threads=1 --nocapture
for profile in devnet mainnet; do
  profile_key=${profile^^}
  path_key="RAYDIUM_SBF_${profile_key}_ELF_PATH"
  hash_key="RAYDIUM_SBF_${profile_key}_ELF_SHA256"
  printf 'Locally compiled pinned Raydium SBF profile: %s\n' "$profile"
  CARGO_TARGET_DIR="$host_target_root/$profile" \
    RAYDIUM_SBF_ELF_PATH="${!path_key}" RAYDIUM_SBF_ELF_SHA256="${!hash_key}" \
    cargo test --locked --offline \
      --manifest-path "$repository_root/tests/amm-v4-source-gate/Cargo.toml" \
      --test funded_source --no-default-features \
      --features "$profile,mint-authority-guard,sbf-raydium" \
      --jobs 3 -- --test-threads=1 --nocapture
done
