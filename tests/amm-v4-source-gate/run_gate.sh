#!/usr/bin/env bash
set -eu
gate_root=$(cd -- "$(dirname -- "$0")" && pwd)
: "${CARGO_TARGET_DIR:?Set CARGO_TARGET_DIR to an isolated build-output directory}"
source_repository=${1:?Pass the verified Raydium Git checkout path}
shift
BEAM_SOURCE_HEAD=$(git -C "$gate_root" rev-parse HEAD)
export BEAM_SOURCE_HEAD
printf 'Beam source head: %s\n' "$BEAM_SOURCE_HEAD"
python3 "$gate_root/prepare_source.py" --repository "$source_repository" --check
cargo test --locked --offline --manifest-path "$gate_root/Cargo.toml" \
  --test funded_source --jobs 3 "$@" -- --test-threads=1 --nocapture
python3 "$gate_root/prepare_source.py" --repository "$source_repository" --check
