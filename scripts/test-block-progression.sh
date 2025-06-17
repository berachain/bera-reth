#!/bin/bash
# Test BeaconKit + bera-reth block progression
set -ex

BEACON_KIT_PATH="${BEACON_KIT_PATH:-../beacon-kit}"
TARGET_BLOCK="${TARGET_BLOCK:-10}"
TIMEOUT="${TIMEOUT:-120}"

[ ! -d "$BEACON_KIT_PATH" ] && { echo "ERROR: Set BEACON_KIT_PATH"; exit 1; }

# Check if required ports are available
for port in 8545 8551 30303 3500; do
    if lsof -i :$port >/dev/null 2>&1; then
        echo "ERROR: Port $port is in use"
        exit 1
    fi
done

cleanup() { 
    echo "Cleaning up processes..."
    pkill -f "beacond\|bera-reth" 2>/dev/null || true
    pkill -f "make start" 2>/dev/null || true
    pkill -f "make start-bera-reth-local" 2>/dev/null || true
    jobs -p | xargs -r kill 2>/dev/null || true
}
trap cleanup EXIT INT TERM

get_block() {
    curl -s -X POST -H "Content-Type: application/json" \
         --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
         http://localhost:8545 2>/dev/null | \
    grep -o '"result":"[^"]*"' | cut -d'"' -f4 | xargs printf "%d\n" 2>/dev/null || echo "0"
}

echo "Testing block progression to $TARGET_BLOCK (timeout: ${TIMEOUT}s)"

# Clean and start BeaconKit
rm -rf /.tmp/beacond ~/.bera-reth 2>/dev/null || true
cd "$BEACON_KIT_PATH"
echo "y" | make start 2>&1 | sed 's/^/[BEACONKIT] /' &
sleep 5

# Verify genesis file exists
[ ! -f "$BEACON_KIT_PATH/.tmp/beacond/eth-genesis.json" ] && { echo "ERROR: Genesis file not found"; exit 1; }

# Start bera-reth
cd - >/dev/null
BEACON_KIT="$BEACON_KIT_PATH" make start-bera-reth-local 2>&1 | sed 's/^/[RETH] /' &
sleep 5

# Monitor block progression
start_time=$(date +%s)
prev_block=0

while [ $(($(date +%s) - start_time)) -lt $TIMEOUT ]; do
    current_block=$(get_block)
    
    if [ "$current_block" -gt "$prev_block" ]; then
        echo "Block: $prev_block -> $current_block"
        prev_block=$current_block
        
        [ "$current_block" -ge "$TARGET_BLOCK" ] && {
            echo "SUCCESS: Reached block $current_block in $(($(date +%s) - start_time))s"
            exit 0
        }
    fi
    
    sleep 3
done

echo "TIMEOUT: Only reached block $prev_block"
exit 1