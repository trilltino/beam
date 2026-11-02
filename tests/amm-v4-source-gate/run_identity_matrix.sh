#!/usr/bin/env bash
set -eu
gate_root=$(cd -- "$(dirname -- "$0")" && pwd)
: "${CARGO_TARGET_DIR:?Set CARGO_TARGET_DIR to an isolated matrix output directory}"
source_repository=${1:?Pass the verified Raydium Git checkout path}
profile_target_root=$CARGO_TARGET_DIR
for profile in devnet mainnet; do
  printf 'Identity matrix profile: %s\n' "$profile"
  CARGO_TARGET_DIR="$profile_target_root/$profile" \
    bash "$gate_root/run_gate.sh" "$source_repository" \
      --no-default-features --features "$profile,mint-authority-guard"
done
