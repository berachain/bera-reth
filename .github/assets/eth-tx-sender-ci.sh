#!/bin/bash
set -euo pipefail  # Exit on any error, undefined variables, pipe failures
IFS=$'\n\t'        # Safer word splitting

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

# Single RPC endpoint and private key
RPC_URL="https://rpc.berachain.com"
PRIVATE_KEY="$CI_PRIVATE_KEY"

# Derive address once at startup
ADDRESS=$(cast wallet address --private-key "$PRIVATE_KEY" 2>/dev/null)
if [[ -z "$ADDRESS" ]]; then
    echo "ERROR: Failed to derive address for private key"
    exit 1
fi

# Transaction recipient (burn address)
TO_ADDRESS="0x0000000000000000000000000000000000000000"

# No color codes needed for CI

# Interval in seconds
INTERVAL=15

# CI flags
FAILED_TXS=()
SLACK_WEBHOOK="${SLACK_WEBHOOK_URL:-}"

echo "🚀 Transaction Sender - sending every ${INTERVAL}s"
echo "📋 Target: $TO_ADDRESS"
echo "💰 Value: 0 wei"
echo "🔗 RPC: $RPC_URL"
echo "💳 Address: $ADDRESS"
echo

# Function to get nonce with pending tag via RPC
get_pending_nonce() {
    # Get pending nonce via direct RPC call
    local response=$(curl -s -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionCount\",\"params\":[\"$ADDRESS\",\"pending\"],\"id\":1}" \
        --connect-timeout 5 --max-time 10 \
        "$RPC_URL" 2>/dev/null)

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


# Function to send a single transaction
send_transaction() {
    local nonce=$1
    local timestamp=$(date '+%H:%M:%S')

    # Extract RPC name for display
    local rpc_name=$(echo "$RPC_URL" | sed 's|.*://||' | cut -d'/' -f1)

    # Build and sign transaction once
    local raw_tx=$(cast mktx "$TO_ADDRESS" \
        --value 0 \
        --private-key "$PRIVATE_KEY" \
        --nonce "$nonce" \
        --gas-limit 21000 \
        --gas-price 100gwei \
        --priority-gas-price 1.1gwei \
        --rpc-url "$RPC_URL"
        2>/dev/null)

    if [[ -z "$raw_tx" ]]; then
        echo "[$timestamp] ${rpc_name} - nonce:$nonce addr:$ADDRESS → MKTX_FAILED"
        return 1
    fi

    # Pre-calculate transaction hash from raw transaction
    local precalc_hash=$(cast keccak "$raw_tx")

    # Send raw transaction via direct RPC call
    local response=$(curl -i -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_sendRawTransaction\",\"params\":[\"$raw_tx\"],\"id\":1}" \
        --connect-timeout 5 --max-time 10 \
        "$RPC_URL" 2>&1)

    # Extract x-host-id from headers (if present)
    local host_id=$(echo "$response" | grep -o 'x-host-id: [a-zA-Z0-9-]*' | cut -d' ' -f2 || echo "unknown")

    # Check if response contains result (success) or error
    if [[ "$response" == *"\"result\":"* && "$response" != *"\"error\":"* ]]; then
        local tx_hash=$(echo "$response" | grep -o '"result":"0x[a-fA-F0-9]*"' | cut -d'"' -f4)
        echo "[$timestamp] ${rpc_name} - nonce:$nonce addr:$ADDRESS precalc:$precalc_hash host:$host_id → SUCCESS: $tx_hash"
    else
        # Extract just the JSON response, not the curl verbose output
        local json_response=$(echo "$response" | tail -1 | grep -o '{.*}')
        echo "[$timestamp] ${rpc_name} - nonce:$nonce addr:$ADDRESS precalc:$precalc_hash host:$host_id → FAILED: $json_response"

        # Store detailed failure for Slack notification
        local detailed_failure="TRANSACTION FAILURE
RPC: $rpc_name ($RPC_URL)
Time: $timestamp
Nonce: $nonce
Address: $ADDRESS
Precalc Hash: $precalc_hash
Host ID: $host_id
Raw TX: $raw_tx
JSON Response: $json_response
---"
        FAILED_TXS+=("$detailed_failure")
    fi
}

# Function to send a single transaction attempt
send_transaction_attempt() {
    local rpc_name=$(echo "$RPC_URL" | sed 's|.*://||' | cut -d'/' -f1)
    local timestamp=$(date '+%H:%M:%S')

    # Get pending nonce
    local nonce=$(get_pending_nonce)

    if [[ "$nonce" == "ERROR" ]]; then
        echo "[$timestamp] ${rpc_name} - addr:$ADDRESS NONCE_ERROR"
    else
        # Send transaction
        send_transaction "$nonce"
    fi
}

# Number of transactions to send (default 20)
TX_COUNT="${TX_SENDER_COUNT:-20}"
echo "🤖 Sending $TX_COUNT transactions..."
echo

# Send specified number of transactions
for i in $(seq 1 $TX_COUNT); do
    echo "Transaction $i/$TX_COUNT:"
    send_transaction_attempt

    # Sleep between transactions (except after the last one)
    if [ $i -lt $TX_COUNT ]; then
        sleep "$INTERVAL"
    fi
done

send_slack_notification() {
    echo "Debug: SLACK_WEBHOOK='$SLACK_WEBHOOK', FAILED_TXS count: ${#FAILED_TXS[@]}"
    if [[ -n "$SLACK_WEBHOOK" && ${#FAILED_TXS[@]} -gt 0 ]]; then
        # Create comprehensive failure summary
        local total_failures=${#FAILED_TXS[@]}
        local timestamp=$(date '+%Y-%m-%d %H:%M:%S UTC')

        # Build detailed message
        local message="BERACHAIN RPC TRANSACTION FAILURES DETECTED

Summary: $total_failures transaction(s) failed out of $TX_COUNT attempts
Report Time: $timestamp
CI Run: https://github.com/berachain/bera-reth/actions/runs/$GITHUB_RUN_ID

Failure Details:
"

        # Add each failure with full details
        local failure_count=1
        for failure in "${FAILED_TXS[@]}"; do
            message+="
Failure #$failure_count:
$failure
"
            ((failure_count++))
        done


        # Send to Slack with proper JSON escaping
        local json_message=$(echo "$message" | sed 's/"/\\"/g' | sed ':a;N;$!ba;s/\n/\\n/g')

        curl -X POST -H 'Content-type: application/json' \
            --data "{\"text\":\"$json_message\"}" \
            "$SLACK_WEBHOOK"
    fi
}

# Send Slack notification if there were failures
send_slack_notification

echo "🛑 $TX_COUNT transactions complete."

# Exit with error code if there were failures
if [[ ${#FAILED_TXS[@]} -gt 0 ]]; then
    echo "❌ Run completed with ${#FAILED_TXS[@]} failures"
    exit 1
else
    echo "✅ Run completed successfully with no failures"
    exit 0
fi