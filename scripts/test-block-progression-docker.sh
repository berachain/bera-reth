#!/bin/bash
# Test BeaconKit (Docker) + bera-reth block progression
set -ex

TARGET_BLOCK="${TARGET_BLOCK:-10}"
TIMEOUT="${TIMEOUT:-120}"

# Check if required ports are available
for port in 8545 8551; do
    if lsof -i :$port >/dev/null 2>&1; then
        echo "ERROR: Port $port is in use"
        exit 1
    fi
done

cleanup() { 
    echo "Cleaning up processes..."
    docker stop beaconkit 2>/dev/null || true
    docker rm beaconkit 2>/dev/null || true
    pkill -f "bera-reth" 2>/dev/null || true
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

# Clean directories
rm -rf ~/.bera-reth 2>/dev/null || true

# Wait for BeaconKit container to be ready
echo "Waiting for BeaconKit container to initialize..."
WAIT_TIME=0
while [ $WAIT_TIME -lt 60 ]; do
    if docker ps | grep -q "beaconkit"; then
        echo "BeaconKit container is running"
        break
    fi
    echo "Waiting for container... (${WAIT_TIME}s elapsed)"
    sleep 5
    WAIT_TIME=$((WAIT_TIME + 5))
done

# Check if BeaconKit is responding
echo "Testing BeaconKit connectivity..."
for i in $(seq 1 10); do
    if curl -s http://localhost:26657/status >/dev/null 2>&1; then
        echo "BeaconKit is responsive"
        break
    fi
    echo "Waiting for BeaconKit RPC... (attempt $i)"
    sleep 3
done

# Start bera-reth
echo "Starting bera-reth..."
timeout 60 ./target/debug/bera-reth node \
    --chain dev \
    --http \
    --http.addr "0.0.0.0" \
    --http.port 8545 \
    --http.api eth,net \
    --authrpc.addr "0.0.0.0" \
    --authrpc.port 8551 \
    --datadir ~/.bera-reth \
    --engine.persistence-threshold 0 \
    --engine.memory-block-buffer-target 0 2>&1 | sed 's/^/[RETH] /' &

RETH_PID=$!
echo "Waiting for bera-reth to start..."
sleep 15

# Test if RPC is responsive
echo "Testing RPC connection..."
for i in $(seq 1 10); do
    if curl -s http://localhost:8545 >/dev/null 2>&1; then
        echo "RPC is responsive"
        break
    fi
    echo "Waiting for RPC... (attempt $i)"
    sleep 2
done

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