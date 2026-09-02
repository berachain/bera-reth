#!/usr/bin/env bash
# Synced with reth v2.5.0
# set -x

cd hivetests/

sim="${1}"
limit="${2}"
fixture_variant="${3:-osaka}"

if [[ "${fixture_variant}" == "osaka" && "${sim}" == *"eels"* && "${limit}" == *"tests/amsterdam"* ]]; then
    echo "osaka fixtures do not support amsterdam tests"
    exit 1
fi

log_file="$(mktemp)"
trap 'rm -f "${log_file}"' EXIT

# Use lower parallelism for eels tests to avoid OOM-killing the runner
parallelism=16
if [[ "${sim}" == *"eels"* ]]; then
    parallelism=4
fi

run_hive() {
    # berachain/hive's CLI predates upstream's --sim.limit.exact flag; --sim.limit
    # is already a regex match there.
    hive \
  --sim "${sim}" \
  --sim.limit "${limit}" \
  --sim.parallelism "${parallelism}" \
  --client bera-reth \
  2>&1 | tee "${log_file}" || true
}

check_log() {
    tail -n 1 "${log_file}" | sed -r 's/\x1B\[[0-9;]*[mK]//g'
}

attempt=0
max_attempts=5

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
