#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
REPORT="$ROOT_DIR/docs/component-porting-compliance.md"

if [ ! -f "$REPORT" ]; then
  printf 'missing report: %s\n' "$REPORT" >&2
  exit 1
fi

require() {
  local needle=$1
  if ! grep -Fq "$needle" "$REPORT"; then
    printf 'missing required report text: %s\n' "$needle" >&2
    exit 1
  fi
}

require '# Component porting compliance report'
require '## Status legend'
require '| Component / crate | Cilium parity anchors | Current Rust artifact status | Relevant integration suites | Score |'

components=(
  seriousum-core
  seriousum-config
  seriousum-crypto
  seriousum-kvstore
  seriousum-api
  seriousum-daemon
  seriousum-cli
  seriousum-operator
  seriousum-hubble
  seriousum-clustermesh
  seriousum-auth
  seriousum-proxy
  seriousum-wireguard
  seriousum-cni
  seriousum-bgp
  seriousum-fqdn
  seriousum-envoy
  seriousum-k8s
  seriousum-datapath
  seriousum-ebpf
  seriousum-controller
)

for component in "${components[@]}"; do
  if ! grep -Fq "| \`$component\` |" "$REPORT"; then
    printf 'missing compliance row for %s\n' "$component" >&2
    exit 1
  fi
done

invalid_rows=$(awk -F'|' '
  $2 ~ /^[[:space:]]*`[^`]+`[[:space:]]*$/ {
    score = $6
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", score)
    if (score !~ /^`[1-5]\/5`$/) {
      print $2 " => " score
    }
  }
' "$REPORT")

if [ -n "$invalid_rows" ]; then
  printf 'report contains invalid score rows:\n%s\n' "$invalid_rows" >&2
  exit 1
fi

printf 'component porting compliance report looks consistent: %s\n' "$REPORT"
