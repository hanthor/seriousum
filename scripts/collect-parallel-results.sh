#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
OUTPUT_DIR=${OUTPUT_DIR:-$ROOT_DIR/target/parallel-test-results}

echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║                 Parallel Test Results Aggregation                           ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo ""
echo "Results Directory: $OUTPUT_DIR"
echo ""

if [ ! -d "$OUTPUT_DIR" ]; then
  echo "No results directory found at $OUTPUT_DIR"
  exit 1
fi

cd "$OUTPUT_DIR"

# Aggregate results
echo "📊 Test Results Summary:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

total_passed=0
total_failed=0
total_specs_run=0

for result_file in *-results.log; do
  if [ ! -f "$result_file" ]; then
    continue
  fi
  
  test_name=$(basename "$result_file" -results.log)
  echo ""
  echo "Test Suite: $test_name"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  
  # Extract key metrics
  if grep -q "FAIL.*PASS" "$result_file"; then
    # Extract pass/fail counts
    summary=$(grep -E "Passed|Failed" "$result_file" | tail -5)
    echo "$summary"
    
    passed=$(echo "$summary" | grep -oE "[0-9]+ Passed" | head -1 | awk '{print $1}' || echo "0")
    failed=$(echo "$summary" | grep -oE "[0-9]+ Failed" | head -1 | awk '{print $1}' || echo "0")
    
    ((total_passed += passed))
    ((total_failed += failed))
  fi
  
  # Extract spec count
  specs=$(grep -oE "Ran [0-9]+ of [0-9]+ Specs" "$result_file" | head -1 | awk '{print $2}' || echo "?")
  echo "Specs Run: $specs"
  
  # Look for errors
  errors=$(grep -c "Error\|error\|FAIL" "$result_file" || echo "0")
  if [ "$errors" -gt 0 ]; then
    echo "⚠️  Errors detected: $errors"
  else
    echo "✅ No errors"
  fi
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Overall Summary:"
echo "  Total Passed: $total_passed"
echo "  Total Failed: $total_failed"
echo ""

# Generate aggregated report
report_file="$OUTPUT_DIR/AGGREGATED_RESULTS.md"
cat > "$report_file" << EOF
# Parallel Test Results - $(date '+%Y-%m-%d %H:%M:%S')

## Summary
- **Total Passed**: $total_passed
- **Total Failed**: $total_failed
- **Result Directory**: $OUTPUT_DIR

## Individual Test Suites

EOF

for result_file in *-results.log; do
  if [ ! -f "$result_file" ]; then
    continue
  fi
  
  test_name=$(basename "$result_file" -results.log)
  echo "### $test_name" >> "$report_file"
  echo "" >> "$report_file"
  echo "\`\`\`" >> "$report_file"
  grep -E "Ran.*Specs|PASS|FAIL|Error" "$result_file" | tail -20 >> "$report_file" || true
  echo "\`\`\`" >> "$report_file"
  echo "" >> "$report_file"
  echo "[View full log]($result_file)" >> "$report_file"
  echo "" >> "$report_file"
done

echo "Report: $report_file"
echo ""

# List all result files
echo "📁 Result Files:"
ls -lh *.log 2>/dev/null | awk '{print "  - " $9 " (" $5 ")"}'

echo ""
echo "✅ Results aggregation complete!"
echo ""
echo "To view aggregated report:"
echo "  cat $report_file"
