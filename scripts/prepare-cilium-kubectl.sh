#!/usr/bin/env bash
set -euo pipefail

KUBECTL_ROOT=
K8S_VERSION=
REAL_KUBECTL=${REAL_KUBECTL:-}

usage() {
  cat <<'EOF'
Usage: scripts/prepare-cilium-kubectl.sh --kubectl-root PATH --k8s-version X.Y [--real-kubectl PATH]

Prepare a version-specific kubectl shim for the upstream Cilium test harness.
The shim avoids the harness fallback that downloads kubectl release candidates.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --kubectl-root)
      KUBECTL_ROOT=$2
      shift 2
      ;;
    --k8s-version)
      K8S_VERSION=$2
      shift 2
      ;;
    --real-kubectl)
      REAL_KUBECTL=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$KUBECTL_ROOT" ] || [ -z "$K8S_VERSION" ]; then
  usage >&2
  exit 2
fi

if [ -z "$REAL_KUBECTL" ]; then
  REAL_KUBECTL=$(command -v kubectl || true)
fi

if [ -z "$REAL_KUBECTL" ]; then
  printf 'kubectl not found in PATH\n' >&2
  exit 1
fi

REAL_KUBECTL=$(readlink -f "$REAL_KUBECTL")
MAJOR=${K8S_VERSION%%.*}
MINOR=${K8S_VERSION#*.}
TARGET_DIR="$KUBECTL_ROOT/$K8S_VERSION"
TARGET_BIN="$TARGET_DIR/kubectl"

mkdir -p "$TARGET_DIR"

cat >"$TARGET_BIN" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if [ "\${1:-}" = "version" ]; then
  has_client=0
  has_json=0
  expect_output_value=0

  for arg in "\$@"; do
    if [ "\$expect_output_value" = "1" ]; then
      if [ "\$arg" = "json" ]; then
        has_json=1
      fi
      expect_output_value=0
      continue
    fi

    case "\$arg" in
      --client|--client=true)
        has_client=1
        ;;
      -o|--output)
        expect_output_value=1
        ;;
      -o=json|--output=json)
        has_json=1
        ;;
    esac
  done

  if [ "\$has_client" = "1" ] && [ "\$has_json" = "1" ]; then
    printf '{"clientVersion":{"major":"%s","minor":"%s"}}\n' "$MAJOR" "$MINOR"
    exit 0
  fi
fi

exec "$REAL_KUBECTL" "\$@"
EOF

chmod +x "$TARGET_BIN"
printf '%s\n' "$TARGET_BIN"
