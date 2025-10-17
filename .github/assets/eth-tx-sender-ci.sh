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
RECEIPT_TIMEOUTS=()
SLACK_WEBHOOK="${SLACK_WEBHOOK_URL:-}"
MAX_WAIT_SEC="${MAX_WAIT_SEC:-30}"

echo "🚀 Transaction Sender - sending every ${INTERVAL}s"
echo "📋 Target: $TO_ADDRESS"
echo "💰 Value: 0 wei"
echo "🔗 RPC: $RPC_URL"
echo "💳 Address: $ADDRESS"
echo "⏱️  Receipt timeout: ${MAX_WAIT_SEC}s"
echo


# Function to poll for transaction receipt
poll_for_receipt() {
    local tx_hash=$1
    local start_time=$2  # Passed from submission time
    local use_precision=$3

    local deadline=$(($(date +%s) + MAX_WAIT_SEC))
    local rpc_name=$(echo "$RPC_URL" | sed 's|.*://||' | cut -d'/' -f1)
    local poll_count=0

    while [ $(date +%s) -lt $deadline ]; do
        ((poll_count++))
        local timestamp=$(date '+%H:%M:%S')

        # Get transaction receipt via RPC
        local response=$(curl -s -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionReceipt\",\"params\":[\"$tx_hash\"],\"id\":1}" \
            --connect-timeout 10 --max-time 30 \
            "$RPC_URL" 2>/dev/null)

        if [[ $? -eq 0 && -n "$response" ]]; then
            # Check for RPC errors first
            if [[ "$response" == *"\"error\":"* ]]; then
                # Log ephemeral RPC error but continue polling
                echo "[$timestamp] ${rpc_name} - Poll #${poll_count} - RPC error: $response" >&2
            # Check if result exists (not null) - we got a receipt
            elif [[ "$response" == *"\"result\":{\"type\""* ]] || [[ "$response" == *"\"result\":{\"transactionHash\""* ]]; then
                local block_number=$(echo "$response" | grep -o '"blockNumber":"0x[a-fA-F0-9]*"' | cut -d'"' -f4)

                # Calculate latency based on available precision
                if [[ $use_precision -eq 1 ]]; then
                    local end_time=$(date +%s.%N)
                    local latency=$(echo "scale=3; $end_time - $start_time" | bc)
                else
                    local end_time=$(date +%s)
                    local latency=$((end_time - start_time))
                fi

                # Convert hex block number to decimal for display
                local block_decimal=$(printf "%d" "$block_number" 2>/dev/null || echo "unknown")

                echo "[$timestamp] ${rpc_name} - Poll #${poll_count} - Receipt found in block $block_decimal" >&2
                echo "INCLUDED|$latency|$block_decimal"
                return 0
            elif [[ "$response" == *"\"result\":null"* ]]; then
                # Log full response when receipt not found
                echo "[$timestamp] ${rpc_name} - Poll #${poll_count} - Receipt not found: $response" >&2
            else
                # Log unexpected response format
                echo "[$timestamp] ${rpc_name} - Poll #${poll_count} - Unexpected response: $response" >&2
            fi
        else
            echo "[$timestamp] ${rpc_name} - Poll #${poll_count} - No response or curl error" >&2
        fi

        # Sleep 250ms between polls
        sleep 0.25
    done

    # Timeout reached
    echo "TIMEOUT|$MAX_WAIT_SEC|0"
    return 1
}

# Function to send a single transaction
send_transaction() {
    local timestamp=$(date '+%H:%M:%S')

    # Extract RPC name for display
    local rpc_name=$(echo "$RPC_URL" | sed 's|.*://||' | cut -d'/' -f1)

    # Build and sign transaction - let RPC determine nonce and gas (like Python)
    local raw_tx=$(cast mktx "$TO_ADDRESS" \
        --value 0 \
        --private-key "$PRIVATE_KEY" \
        --rpc-url "$RPC_URL" \
        2>/dev/null)

    if [[ -z "$raw_tx" ]]; then
        echo "[$timestamp] ${rpc_name} - addr:$ADDRESS → MKTX_FAILED"
        return 1
    fi

    # Decode and log transaction details
    local tx_details=$(cast decode-transaction "$raw_tx" 2>/dev/null)
    echo "[$timestamp] ${rpc_name} - Decoded TX: $tx_details"

    # Pre-calculate transaction hash from raw transaction
    local precalc_hash=$(cast keccak "$raw_tx")

    # Track submission time for latency calculation
    local use_precision=0
    if command -v bc &>/dev/null && [[ "$(date +%N 2>/dev/null)" != "%N" ]]; then
        local submit_time=$(date +%s.%N)
        use_precision=1
    else
        local submit_time=$(date +%s)
    fi

    # Send raw transaction via direct RPC call
    local response=$(curl -i -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_sendRawTransaction\",\"params\":[\"$raw_tx\"],\"id\":1}" \
        --connect-timeout 10 --max-time 30 \
        "$RPC_URL" 2>&1)

    # Extract x-host-id from headers (if present)
    local host_id=$(echo "$response" | grep -o 'x-host-id: [a-zA-Z0-9-]*' | cut -d' ' -f2 || echo "unknown")

    # Check if response contains result (success) or error
    if [[ "$response" == *"\"result\":"* && "$response" != *"\"error\":"* ]]; then
        local tx_hash=$(echo "$response" | grep -o '"result":"0x[a-fA-F0-9]*"' | cut -d'"' -f4)
        echo "[$timestamp] ${rpc_name} - addr:$ADDRESS precalc:$precalc_hash host:$host_id → SUBMITTED: $tx_hash"

        # Poll for receipt
        local receipt_result=$(poll_for_receipt "$tx_hash" "$submit_time" "$use_precision")
        IFS='|' read -r receipt_status receipt_latency block_number <<< "$receipt_result"

        if [[ "$receipt_status" == "TIMEOUT" ]]; then
            echo "[$timestamp] ${rpc_name} - tx:$tx_hash → RECEIPT_TIMEOUT after ${MAX_WAIT_SEC}s"
            RECEIPT_TIMEOUTS+=("TX: $tx_hash | RPC: $rpc_name | Time: $timestamp | Host: $host_id")
        elif [[ "$receipt_status" == "INCLUDED" ]]; then
            echo "[$timestamp] ${rpc_name} - tx:$tx_hash → INCLUDED: block=$block_number latency=${receipt_latency}s"
        fi
    else
        # Show full response for debugging
        echo "[$timestamp] ${rpc_name} - addr:$ADDRESS precalc:$precalc_hash host:$host_id → FAILED"
        echo "Full response: $response"

        # Store detailed failure for Slack notification
        local detailed_failure="TRANSACTION FAILURE
RPC: $rpc_name ($RPC_URL)
Time: $timestamp
Address: $ADDRESS
Precalc Hash: $precalc_hash
Host ID: $host_id
Raw TX: $raw_tx
Full Response: $response
---"
        FAILED_TXS+=("$detailed_failure")
    fi
}

# Function to send a single transaction attempt
send_transaction_attempt() {
    # Send transaction (nonce determined by RPC)
    send_transaction
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
    echo "Debug: SLACK_WEBHOOK='$SLACK_WEBHOOK', FAILED_TXS count: ${#FAILED_TXS[@]}, RECEIPT_TIMEOUTS count: ${#RECEIPT_TIMEOUTS[@]}"
    if [[ -n "$SLACK_WEBHOOK" && ( ${#FAILED_TXS[@]} -gt 0 || ${#RECEIPT_TIMEOUTS[@]} -gt 0 ) ]]; then
        # Create comprehensive failure summary
        local total_failures=${#FAILED_TXS[@]}
        local total_timeouts=${#RECEIPT_TIMEOUTS[@]}
        local timestamp=$(date '+%Y-%m-%d %H:%M:%S UTC')

        # Build detailed message
        local message="BERACHAIN RPC TRANSACTION ISSUES DETECTED

Summary:
- Submission Failures: $total_failures
- Receipt Timeouts: $total_timeouts
- Total Attempts: $TX_COUNT
Report Time: $timestamp
CI Run: https://github.com/berachain/bera-reth/actions/runs/$GITHUB_RUN_ID
"

        # Add submission failures
        if [[ ${#FAILED_TXS[@]} -gt 0 ]]; then
            message+="
==== SUBMISSION FAILURES ====
"
            local failure_count=1
            for failure in "${FAILED_TXS[@]}"; do
                message+="
Failure #$failure_count:
$failure
"
                ((failure_count++))
            done
        fi

        # Add receipt timeouts
        if [[ ${#RECEIPT_TIMEOUTS[@]} -gt 0 ]]; then
            message+="
==== RECEIPT TIMEOUTS ====
"
            local timeout_count=1
            for timeout in "${RECEIPT_TIMEOUTS[@]}"; do
                message+="
Timeout #$timeout_count:
$timeout
"
                ((timeout_count++))
            done
        fi

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

# Exit with error code if there were failures or timeouts
if [[ ${#FAILED_TXS[@]} -gt 0 || ${#RECEIPT_TIMEOUTS[@]} -gt 0 ]]; then
    echo "❌ Run completed with ${#FAILED_TXS[@]} failures and ${#RECEIPT_TIMEOUTS[@]} receipt timeouts"
    exit 1
else
    echo "✅ Run completed successfully with no failures or timeouts"
    exit 0
fi