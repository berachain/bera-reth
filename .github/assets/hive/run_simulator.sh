#!/usr/bin/env bash
# Copied from reth at commit 91730cd326f6b0f975e56d8985f08a2a6c943f89
# set -x

cd hivetests/

sim="${1}"
limit="${2}"

run_hive() {
    hive --sim "${sim}" --sim.limit "${limit}" --sim.parallelism 8 --client bera-reth 2>&1 | tee /tmp/log || true
}

check_log() {
    tail -n 1 /tmp/log | sed -r 's/\x1B\[[0-9;]*[mK]//g'
}

attempt=0
max_attempts=30

while [ $attempt -lt $max_attempts ]; do
    run_hive

    # Check if no tests were run. sed removes ansi colors
    if check_log | grep -q "suites=0"; then
        echo "no tests were run, retrying in 10 seconds"
        sleep 10
        attempt=$((attempt + 1))
        continue
    fi

    # Check the last line of the log for "finished", "tests failed", or "test failed"
    if check_log | grep -Eq "(finished|tests? failed)"; then
        exit 0
    else
        exit 1
    fi
done
exit 1
