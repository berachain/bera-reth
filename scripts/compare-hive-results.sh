#!/bin/bash

# Hive RPC Test Results Comparison Script
# Compares test results between bera-reth and reth, analyzing both test names and failure reasons

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="${1:-workspace/logs}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

usage() {
    echo "Usage: $0 [results_directory]"
    echo ""
    echo "Compare hive RPC test results between bera-reth and reth"
    echo ""
    echo "IMPORTANT: Run this script from the hive directory after running tests"
    echo ""
    echo "Example workflow:"
    echo "  cd /path/to/hive"
    echo "  ./hive --sim ethereum/rpc-compat --client bera-reth,reth"
    echo "  ../bera-reth/scripts/compare-hive-results.sh"
    echo ""
    echo "The script defaults to ./workspace/logs (relative to current directory)"
    echo ""
    echo "Alternative usage:"
    echo "  $0 /custom/path/to/results"
    exit 1
}

if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    usage
fi

# Find the combined results JSON file
RESULTS_JSON=$(find "$RESULTS_DIR" -name "*.json" ! -name "hive.json" 2>/dev/null | head -1)

if [[ ! -f "$RESULTS_JSON" ]]; then
    echo -e "${RED}❌ No results JSON found in $RESULTS_DIR${NC}"
    echo ""
    echo "Make sure you:"
    echo "1. Run from the hive directory: cd /path/to/hive"
    echo "2. Have run: ./hive --sim ethereum/rpc-compat --client bera-reth,reth"
    echo "3. Then run: ../bera-reth/scripts/compare-hive-results.sh"
    exit 1
fi

echo -e "${YELLOW}📊 Analyzing hive results from: $RESULTS_JSON${NC}"
echo ""

# Extract basic test counts
BERA_RETH_PASSED=$(jq '[.testCases[] | select(.name | contains("(bera-reth)")) | select(.summaryResult.pass == true)] | length' "$RESULTS_JSON")
BERA_RETH_TOTAL=$(jq '[.testCases[] | select(.name | contains("(bera-reth)"))] | length' "$RESULTS_JSON")
BERA_RETH_FAILED=$((BERA_RETH_TOTAL - BERA_RETH_PASSED))

RETH_PASSED=$(jq '[.testCases[] | select(.name | contains("(reth)")) | select(.summaryResult.pass == true)] | length' "$RESULTS_JSON")
RETH_TOTAL=$(jq '[.testCases[] | select(.name | contains("(reth)"))] | length' "$RESULTS_JSON")
RETH_FAILED=$((RETH_TOTAL - RETH_PASSED))

echo "📋 Test Summary"
echo "| Client        | Passed | Failed | Total |"
echo "|---------------|--------|--------|-------|"
echo "| bera-reth     | $BERA_RETH_PASSED | $BERA_RETH_FAILED | $BERA_RETH_TOTAL |"
echo "| reth:nightly  | $RETH_PASSED | $RETH_FAILED | $RETH_TOTAL |"
echo ""

# Create temporary files for comparison
TEMP_DIR=$(mktemp -d)
BERA_FAILURES="$TEMP_DIR/bera_failures.json"
RETH_FAILURES="$TEMP_DIR/reth_failures.json"

# Extract failed test details (name + failure info)
jq -r '.testCases[] | select(.name | contains("(bera-reth)")) | select(.summaryResult.pass == false) | {name: (.name | sub(" \\(bera-reth\\)$"; "")), log_begin: .summaryResult.log.begin, log_end: .summaryResult.log.end}' "$RESULTS_JSON" > "$BERA_FAILURES"

jq -r '.testCases[] | select(.name | contains("(reth)")) | select(.summaryResult.pass == false) | {name: (.name | sub(" \\(reth\\)$"; "")), log_begin: .summaryResult.log.begin, log_end: .summaryResult.log.end}' "$RESULTS_JSON" > "$RETH_FAILURES"

# Get just the test names for simple comparison
jq -r '.name' "$BERA_FAILURES" | sort > "$TEMP_DIR/bera_failed_names.txt"
jq -r '.name' "$RETH_FAILURES" | sort > "$TEMP_DIR/reth_failed_names.txt"

# Compare test names first
NAMES_IDENTICAL=true
if ! cmp -s "$TEMP_DIR/bera_failed_names.txt" "$TEMP_DIR/reth_failed_names.txt"; then
    NAMES_IDENTICAL=false
fi

# Compare test names
if [[ "$NAMES_IDENTICAL" == "true" ]]; then
    echo -e "${GREEN}✅ SUCCESS: bera-reth and reth:nightly fail the same tests${NC}"
    OVERALL_STATUS="success"
else
    echo -e "${RED}❌ bera-reth and reth:nightly fail different tests${NC}"
    echo ""
    
    # Show which tests fail only in bera-reth
    BERA_ONLY=$(comm -23 "$TEMP_DIR/bera_failed_names.txt" "$TEMP_DIR/reth_failed_names.txt")
    if [[ -n "$BERA_ONLY" ]]; then
        echo "🔴 Tests failing only in bera-reth:"
        echo "$BERA_ONLY" | sed 's/^/  - /'
        echo ""
    fi
    
    # Show which tests fail only in reth
    RETH_ONLY=$(comm -13 "$TEMP_DIR/bera_failed_names.txt" "$TEMP_DIR/reth_failed_names.txt")
    if [[ -n "$RETH_ONLY" ]]; then
        echo "🟡 Tests failing only in reth:nightly:"
        echo "$RETH_ONLY" | sed 's/^/  - /'
        echo ""
    fi
    
    OVERALL_STATUS="failure"
fi

echo "📁 Results located in: $RESULTS_DIR"
echo "💡 To reproduce: hive --sim ethereum/rpc-compat --client bera-reth,reth"

# Cleanup
rm -rf "$TEMP_DIR"

# Exit with appropriate code
if [[ "$OVERALL_STATUS" == "success" ]]; then
    exit 0
else
    exit 1
fi