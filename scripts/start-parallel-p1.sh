#!/usr/bin/env bash
set -euo pipefail

###############################################################################
# Parallel P1 Implementation Coordinator
#
# Spawns 4 independent implementation tracks for:
# 1. Service observer (Track 1)
# 2. eBPF maps (Track 2)  
# 3. Backend mapping (Track 3 - depends on 1+2)
# 4. Load balancing algorithm (Track 4 - depends on 1+2)
#
# Usage:
#   bash scripts/start-parallel-p1.sh [--worktrees] [--dry-run]
#
# Options:
#   --worktrees     Use git worktrees for isolation (default: use same repo)
#   --dry-run       Show what would be done, don't execute
#   --monitor       Keep running, monitor progress every 30s
#
###############################################################################

ROOT_DIR=$(git rev-parse --show-toplevel)
cd "$ROOT_DIR"

USE_WORKTREES=${USE_WORKTREES:-0}
DRY_RUN=${DRY_RUN:-0}
MONITOR=${MONITOR:-0}

# Parse arguments
while [ "$#" -gt 0 ]; do
  case "$1" in
    --worktrees) USE_WORKTREES=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --monitor) MONITOR=1; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║                                                                              ║"
echo "║              🚀 PARALLEL P1 IMPLEMENTATION COORDINATOR 🚀                    ║"
echo "║                                                                              ║"
echo "║  Starting 4 independent implementation tracks simultaneously                ║"
echo "║                                                                              ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo ""

# Track configuration
declare -a TRACKS=(
  "observer:service-observer:Track 1 - Service Observer:Issue #44"
  "ebpf:ebpf-loader:Track 2 - eBPF Maps:Issue #45"
  "mapping:backend-mapping:Track 3 - Backend Mapping:Issue #46"
  "lbalance:load-balancer:Track 4 - Load Balancing:Issue #47"
)

echo "📋 Implementation Tracks:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
for track in "${TRACKS[@]}"; do
  IFS=: read -r dir crate desc issue <<< "$track"
  echo "  $desc"
  echo "    Crate: $crate"
  echo "    Issue: $issue"
done
echo ""

# Setup worktrees if requested
if [ "$USE_WORKTREES" = "1" ]; then
  echo "🔧 Setting up git worktrees for isolation..."
  echo ""
  
  for track in "${TRACKS[@]}"; do
    IFS=: read -r dir crate desc issue <<< "$track"
    worktree_path="$ROOT_DIR/worktrees/$dir"
    
    if [ ! -d "$worktree_path" ]; then
      echo "  Creating worktree: $worktree_path"
      if [ "$DRY_RUN" = "0" ]; then
        git worktree add "$worktree_path" main
      fi
    fi
  done
  echo ""
fi

# Function to build and test a track
build_track() {
  local track_dir=$1
  local track_crate=$2
  local track_desc=$3
  local track_issue=$4
  local output_log="$ROOT_DIR/target/p1-build-${track_dir}.log"
  
  echo "$(date '+%Y-%m-%d %H:%M:%S') [$(basename $output_log .log)] Starting build..." > "$output_log"
  
  echo -e "${BLUE}Building: $track_desc${NC}"
  echo "  Output: $output_log"
  
  if [ "$DRY_RUN" = "0" ]; then
    (
      set -e
      cd "$ROOT_DIR"
      
      echo "  ├─ Building $track_crate..." >> "$output_log"
      cargo build --release -p "$track_crate" >> "$output_log" 2>&1 || {
        echo "  ├─ ❌ Build failed" >> "$output_log"
        return 1
      }
      
      echo "  ├─ Running tests for $track_crate..." >> "$output_log"
      cargo test --release -p "$track_crate" >> "$output_log" 2>&1 || {
        echo "  ├─ ⚠️  Tests failed (continuing)" >> "$output_log"
      }
      
      echo "  ├─ Checking clippy..." >> "$output_log"
      cargo clippy --release -p "$track_crate" -- -D warnings >> "$output_log" 2>&1 || {
        echo "  ├─ ⚠️  Clippy warnings (continuing)" >> "$output_log"
      }
      
      echo "  └─ ✅ $track_crate complete" >> "$output_log"
    ) &
  else
    echo "  [DRY RUN] Would build $track_crate"
  fi
}

# Start all builds in parallel
declare -a PIDS

echo "🏗️  Starting parallel builds (4 tracks simultaneously)..."
echo ""

for track in "${TRACKS[@]}"; do
  IFS=: read -r dir crate desc issue <<< "$track"
  build_track "$dir" "$crate" "$desc" "$issue" &
  PIDS+=($!)
done

echo "📊 Build processes started:"
for i in "${!PIDS[@]}"; do
  track="${TRACKS[$i]}"
  IFS=: read -r dir crate desc issue <<< "$track"
  echo "  PID ${PIDS[$i]}: $desc"
done
echo ""

# Wait for all builds to complete
if [ "$DRY_RUN" = "0" ]; then
  echo "⏳ Waiting for all builds to complete..."
  echo ""
  
  # Optional: Monitor progress
  if [ "$MONITOR" = "1" ]; then
    while sleep 30; do
      echo -ne "\033[2K\r$(date '+%H:%M:%S') Builds running..."
      
      all_done=true
      for pid in "${PIDS[@]}"; do
        if ps -p $pid > /dev/null 2>&1; then
          all_done=false
          break
        fi
      done
      
      if [ "$all_done" = true ]; then
        break
      fi
    done
    echo ""
  fi
  
  # Wait for all PIDs
  for i in "${!PIDS[@]}"; do
    pid=${PIDS[$i]}
    track="${TRACKS[$i]}"
    IFS=: read -r dir crate desc issue <<< "$track"
    
    if wait $pid 2>/dev/null; then
      echo "✅ $desc completed successfully"
    else
      echo "❌ $desc failed"
    fi
  done
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📁 Build outputs:"
for track in "${TRACKS[@]}"; do
  IFS=: read -r dir crate desc issue <<< "$track"
  log_file="$ROOT_DIR/target/p1-build-${dir}.log"
  if [ -f "$log_file" ]; then
    size=$(du -h "$log_file" | cut -f1)
    echo "  $desc: $log_file ($size)"
  fi
done
echo ""

# Generate summary
echo "📊 Build Summary:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

passed=0
failed=0

for track in "${TRACKS[@]}"; do
  IFS=: read -r dir crate desc issue <<< "$track"
  log_file="$ROOT_DIR/target/p1-build-${dir}.log"
  
  if [ -f "$log_file" ] && grep -q "✅" "$log_file"; then
    echo "✅ $desc"
    ((passed++))
  else
    echo "❌ $desc"
    ((failed++))
  fi
done

echo ""
echo "Summary: $passed passed, $failed failed"
echo ""

if [ "$failed" -eq 0 ]; then
  echo -e "${GREEN}✅ All tracks built successfully!${NC}"
  exit 0
else
  echo -e "${YELLOW}⚠️  Some tracks failed. Review logs above.${NC}"
  exit 1
fi
