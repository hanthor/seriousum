#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
cd "$ROOT_DIR"

OUTPUT_DIR=${1:-target/cilium-dropin}
TARGET_DIR=${CARGO_TARGET_DIR:-target}
RELEASE_DIR="$TARGET_DIR/release"

cargo build --release --workspace --bins

release_dir_abs=$(cd "$RELEASE_DIR" && pwd)

mkdir -p "$OUTPUT_DIR"

make_alias() {
  alias_name=$1
  source_name=$2
  source_path=$release_dir_abs/$source_name
  alias_path=$OUTPUT_DIR/$alias_name

  if [ ! -x "$source_path" ]; then
    printf 'missing built artifact: %s\n' "$source_path" >&2
    exit 1
  fi

  rm -f "$alias_path"
  ln -s "$source_path" "$alias_path"
}

make_alias cilium seriousum-daemon
make_alias cilium-dbg seriousum-daemon
make_alias cilium-cli seriousum-cli
make_alias cilium-cni seriousum-cni
make_alias operator seriousum-operator
make_alias hubble hubble
make_alias clustermesh-apiserver clustermesh-apiserver

printf 'Created Cilium drop-in aliases:\n'
printf '  %s\n' \
  "$OUTPUT_DIR/cilium" \
  "$OUTPUT_DIR/cilium-dbg" \
  "$OUTPUT_DIR/cilium-cli" \
  "$OUTPUT_DIR/cilium-cni" \
  "$OUTPUT_DIR/operator" \
  "$OUTPUT_DIR/hubble" \
  "$OUTPUT_DIR/clustermesh-apiserver"
