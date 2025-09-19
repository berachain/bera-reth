#!/bin/bash
set -e  # Exit on any error

# Load private key from environment (required for CI, optional for manual runs)
if [[ -z "$CI_PRIVATE_KEY" ]]; then
    if [[ -t 0 ]]; then
        echo "ERROR: CI_PRIVATE_KEY environment variable not set"
        echo "For manual testing, set: export CI_PRIVATE_KEY=your_private_key"
        exit 1
    else
        echo "ERROR: CI_PRIVATE_KEY environment variable not set"
        exit 1
    fi
fi

# RPC endpoints and corresponding private keys
RPC_URLS=(
    "https://rpc.berachain.com"
)

PRIVATE_KEYS=(
    "$CI_PRIVATE_KEY"
)

# Derive addresses once at startup
ADDRESSES=()
for private_key in "${PRIVATE_KEYS[@]}"; do
    address=$(cast wallet address --private-key "$private_key" 2>/dev/null)
    if [[ -z "$address" ]]; then
        echo "ERROR: Failed to derive address for private key ${private_key:0:8}..."
        exit 1
    fi
    ADDRESSES+=("$address")
done

# Transaction recipient (burn address)
TO_ADDRESS="0x0000000000000000000000000000000000000000"

# Transaction value in wei
TX_VALUE="0"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Interval in seconds (CI mode: faster testing)
INTERVAL=30

# CI flags
FAILED_TXS=()
SLACK_WEBHOOK="${SLACK_WEBHOOK_URL:-}"

echo "🚀 Transaction Sender - sending every ${INTERVAL}s (Ctrl+C to stop)..."
echo "📋 Target: $TO_ADDRESS"
echo "💰 Value: $TX_VALUE wei"
echo

# Function to get nonce with pending tag via RPC
get_pending_nonce() {
    local rpc_url=$1
    local address=$2

    # Get pending nonce via direct RPC call
    local response=$(curl -s -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionCount\",\"params\":[\"$address\",\"pending\"],\"id\":1}" \
        --connect-timeout 5 --max-time 10 \
        "$rpc_url" 2>/dev/null)

    if [[ $? -eq 0 && -n "$response" ]]; then
        # Extract hex nonce and convert to decimal
        local hex_nonce=$(echo "$response" | grep -o '"result":"0x[a-fA-F0-9]*"' | cut -d'"' -f4)
        if [[ -n "$hex_nonce" ]]; then
            local decimal_nonce=$(printf "%d" "$hex_nonce" 2>/dev/null || echo "ERROR")
            echo "$decimal_nonce"
        else
            echo "ERROR"
        fi
    else
        echo "ERROR"
    fi
}

# Function to query nonces concurrently
query_concurrent_nonces() {
    local rpc_url=$1
    local address=$2
    local rpc_name=$(echo "$rpc_url" | sed 's|.*://||' | cut -d'/' -f1)

    # Create temp directory for concurrent results
    local temp_dir=$(mktemp -d)

    # Launch 10 concurrent nonce queries
    for i in {1..10}; do
        (get_pending_nonce "$rpc_url" "$address" > "$temp_dir/$i") &
    done

    # Wait for all queries to complete
    wait

    # Collect results into single line
    local nonces=()
    for i in {1..10}; do
        nonces+=($(cat "$temp_dir/$i"))
    done

    # Clean up temp directory
    rm -rf "$temp_dir"

    # Log results in single line
    local timestamp=$(date '+%H:%M:%S')
    echo -e "${BLUE}[$timestamp] ${rpc_name}${NC} - concurrent nonces: ${nonces[*]}"
}

# Function to send a single transaction
send_transaction() {
    local rpc_url=$1
    local private_key=$2
    local address=$3
    local nonce=$4
    local timestamp=$(date '+%H:%M:%S')

    # Extract RPC name for display
    local rpc_name=$(echo "$rpc_url" | sed 's|.*://||' | cut -d'/' -f1)

    # Mask private key for logging (show first 8 chars + ...)
    local masked_key="${private_key:0:8}..."

    # Build and sign transaction once
    local raw_tx=$(cast mktx "$TO_ADDRESS" \
        --value 0 \
        --private-key "$private_key" \
        --nonce "$nonce" \
        --gas-limit 21000 \
        --gas-price 1.1gwei \
        --priority-gas-price 1.1gwei \
        --rpc-url "$rpc_url"
        2>/dev/null)

    if [[ -z "$raw_tx" ]]; then
        echo -e "${RED}[$timestamp] ${rpc_name}${NC} - nonce:$nonce key:$masked_key → MKTX_FAILED"
        return 1
    fi

    # Pre-calculate transaction hash from raw transaction
    echo "$raw_tx"
    local precalc_hash=$(cast keccak "$raw_tx")

    # Send raw transaction via direct RPC call
    local response=$(curl -v -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_sendRawTransaction\",\"params\":[\"$raw_tx\"],\"id\":1}" \
        --connect-timeout 5 --max-time 10 \
        "$rpc_url" 2>&1)

    # echo "RPC Response: $response"

    # Extract x-host-id from headers
    local host_id=$(echo "$response" | grep -o 'x-host-id: [a-zA-Z0-9-]*' | cut -d' ' -f2)

    # Check if response contains result (success) or error
    if [[ "$response" == *"\"result\":"* ]]; then
        local tx_hash=$(echo "$response" | grep -o '"result":"0x[a-fA-F0-9]*"' | cut -d'"' -f4)
        echo -e "${GREEN}[$timestamp] ${rpc_name}${NC} - nonce:$nonce key:$masked_key precalc:$precalc_hash host:$host_id → SUCCESS: $tx_hash"
        # Query nonces concurrently after successful transaction
        query_concurrent_nonces "$rpc_url" "$address" &
    else
        local failure_msg="[$timestamp] ${rpc_name} - nonce:$nonce key:$masked_key precalc:$precalc_hash host:$host_id → FAILED: $response"
        echo -e "${RED}$failure_msg${NC}"

        # Store detailed failure for Slack notification
        local detailed_failure="🔴 **TRANSACTION FAILURE**
📍 **RPC**: $rpc_name ($rpc_url)
⏰ **Time**: $timestamp
🔢 **Nonce**: $nonce
🔑 **Key**: $masked_key
🧮 **Precalc Hash**: $precalc_hash
🖥️ **Host ID**: $host_id
📤 **Raw TX**: \`$raw_tx\`
📥 **Full Response**: \`$response\`
---"
        FAILED_TXS+=("$detailed_failure")
    fi
}

# Function to process a single RPC
process_rpc() {
    local rpc_url=$1
    local private_key=$2
    local address=$3
    local rpc_name=$(echo "$rpc_url" | sed 's|.*://||' | cut -d'/' -f1)

    while true; do
        local timestamp=$(date '+%H:%M:%S')

        # Get pending nonce
        local nonce=$(get_pending_nonce "$rpc_url" "$address")

        if [[ "$nonce" == "ERROR" ]]; then
            echo -e "${RED}[$timestamp] ${rpc_name}${NC} - NONCE_ERROR"
        else
            # Send transaction in background
            send_transaction "$rpc_url" "$private_key" "$address" "$nonce" &
        fi

        sleep "$INTERVAL"
    done
}

echo "🔑 Private keys configured for ${#RPC_URLS[@]} RPCs"
echo "⚡ Starting parallel transaction sending..."
echo

# Start parallel processes for each RPC
pids=()
for i in "${!RPC_URLS[@]}"; do
    rpc_url="${RPC_URLS[$i]}"
    private_key="${PRIVATE_KEYS[$i]}"
    address="${ADDRESSES[$i]}"
    process_rpc "$rpc_url" "$private_key" "$address" &
    pids+=($!)
done

# Run for 1 hour, then exit and report failures
send_slack_notification() {
    if [[ -n "$SLACK_WEBHOOK" && ${#FAILED_TXS[@]} -gt 0 ]]; then
        # Create comprehensive failure summary
        local total_failures=${#FAILED_TXS[@]}
        local timestamp=$(date '+%Y-%m-%d %H:%M:%S UTC')

        # Build detailed message
        local message="🚨 **BERACHAIN RPC TRANSACTION FAILURES DETECTED** 🚨

📊 **Summary**: $total_failures transaction(s) failed during $((DURATION / 60))-minute monitoring period
🕐 **Report Time**: $timestamp
🔗 **CI Run**: https://github.com/berachain/bera-reth/actions/runs/$GITHUB_RUN_ID

📋 **Failure Details**:
"

        # Add each failure with full details
        local failure_count=1
        for failure in "${FAILED_TXS[@]}"; do
            message+="
**Failure #$failure_count**:
$failure
"
            ((failure_count++))
        done

        message+="
📈 **Monitoring**: This job runs every 3 hours to ensure RPC reliability"

        # Send to Slack with proper JSON escaping
        local json_message=$(echo "$message" | sed 's/"/\\"/g' | sed ':a;N;$!ba;s/\n/\\n/g')

        curl -X POST -H 'Content-type: application/json' \
            --data "{\"text\":\"$json_message\"}" \
            "$SLACK_WEBHOOK"
    fi
}

# Run for 5 minutes (300 seconds) - configurable via environment
DURATION="${TX_SENDER_DURATION:-300}"
echo "🤖 Running for $((DURATION / 60)) minutes..."

# Set trap for cleanup and notification
trap 'echo -e "\n${YELLOW}🛑 Run complete. Cleaning up...${NC}"; for pid in "${pids[@]}"; do kill $pid 2>/dev/null; done; send_slack_notification; exit 0' INT TERM

# Run for specified duration, then cleanup and send notifications
sleep "$DURATION"

echo -e "\n${YELLOW}🛑 $((DURATION / 60)) minutes complete. Stopping transaction sender...${NC}"
for pid in "${pids[@]}"; do kill $pid 2>/dev/null; done

# Send Slack notification if there were failures
send_slack_notification

# Exit with error code if there were failures
if [[ ${#FAILED_TXS[@]} -gt 0 ]]; then
    echo "❌ Run completed with ${#FAILED_TXS[@]} failures"
    exit 1
else
    echo "✅ Run completed successfully with no failures"
    exit 0
fi