#!/usr/bin/env bash
set -eo pipefail

# Create the hive_assets directory
mkdir hive_assets/

cd hivetests
go build .

# Pre-build simulators with the correct fixture versions for prague tests
echo "Building simulators with prague test fixtures"
./hive -client bera-reth --sim "ethereum/eest" --sim.buildarg fixtures=https://github.com/ethereum/execution-spec-tests/releases/download/v4.4.0/fixtures_develop.tar.gz --sim.buildarg branch=v4.4.0 -sim.timelimit 1s || true

mv ./hive ../hive_assets/