#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
DASHBOARD="$ROOT_DIR/docs/PARITY_PROOF_DASHBOARD.md"
STATUS_JSON="$ROOT_DIR/docs/generated/parity-proof.json"

if [[ ! -f "$DASHBOARD" ]]; then
  printf 'missing parity proof dashboard: %s\n' "$DASHBOARD" >&2
  exit 1
fi

if [[ ! -f "$STATUS_JSON" ]]; then
  printf 'missing parity proof status json: %s\n' "$STATUS_JSON" >&2
  exit 1
fi

require_text() {
  local needle=$1
  if ! grep -Fq "$needle" "$DASHBOARD"; then
    printf 'missing required dashboard text: %s\n' "$needle" >&2
    exit 1
  fi
}

require_text '# Seriousum Parity Proof Dashboard'
require_text '## Proof model'
require_text '## Proof scoreboard'
require_text '## Exit criteria for a real “fully reimplemented” claim'
require_text '**Overall result**: ⚠️ **NOT YET PROVEN**'

python3 - "$ROOT_DIR" "$STATUS_JSON" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
status_path = Path(sys.argv[2])
obj = json.loads(status_path.read_text())

required_top = {
    'assessment_date',
    'target_statement',
    'overall_verdict',
    'overall_status',
    'pillars',
    'exit_criteria',
}
missing = required_top - obj.keys()
if missing:
    raise SystemExit(f'missing top-level parity-proof keys: {sorted(missing)}')

if obj['overall_verdict'] != 'not_yet_proven':
    raise SystemExit(f"overall_verdict must be 'not_yet_proven', got {obj['overall_verdict']!r}")

allowed_status = {'green', 'yellow', 'red'}
if obj['overall_status'] not in allowed_status:
    raise SystemExit(f"invalid overall_status: {obj['overall_status']!r}")

required_pillars = {
    'scope_inventory',
    'implementation_coverage',
    'behavioral_test_parity',
    'operational_parity',
    'performance_parity',
    'production_soak_proof',
}
pillars = obj['pillars']
missing_pillars = required_pillars - pillars.keys()
if missing_pillars:
    raise SystemExit(f'missing pillars: {sorted(missing_pillars)}')

for name, pillar in pillars.items():
    for key in ('status', 'summary', 'evidence'):
        if key not in pillar:
            raise SystemExit(f'pillar {name} missing key {key}')
    if pillar['status'] not in allowed_status:
        raise SystemExit(f'pillar {name} has invalid status {pillar["status"]!r}')
    if not isinstance(pillar['evidence'], list):
        raise SystemExit(f'pillar {name} evidence must be a list')
    for rel in pillar['evidence']:
        path = root / rel
        if not path.exists():
            raise SystemExit(f'pillar {name} references missing evidence path: {rel}')

required_exit = {
    'frozen_target_release_recorded',
    'full_scope_inventory_completed',
    'runtime_go_exceptions_zero_for_claimed_scope',
    'unmodified_upstream_integration_matrix_passes',
    'differential_behavior_checks_pass',
    'install_upgrade_rollback_parity_verified',
    'performance_budgets_met',
    'soak_chaos_recovery_evidence_published',
}
missing_exit = required_exit - obj['exit_criteria'].keys()
if missing_exit:
    raise SystemExit(f'missing exit criteria keys: {sorted(missing_exit)}')

for key, value in obj['exit_criteria'].items():
    if not isinstance(value, bool):
        raise SystemExit(f'exit criterion {key} must be boolean, got {type(value).__name__}')

print(f'parity proof artifacts validated: {status_path}')
PY
