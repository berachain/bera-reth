#!/usr/bin/env python3
"""Local hive test runner - parses scenarios from .github/workflows/hive.yml

Usage:
    ./scripts/run-hive-local.py                            # List available scenarios
    ./scripts/run-hive-local.py --all                      # Run all scenarios
    ./scripts/run-hive-local.py smoke/genesis              # Run specific scenario
    ./scripts/run-hive-local.py --skip-build smoke/genesis # Skip docker build
"""

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

import yaml

BERA_RETH_DIR = Path(__file__).parent.resolve().parent
HIVE_WORKFLOW = BERA_RETH_DIR / ".github/workflows/hive.yml"
HIVE_DIR = BERA_RETH_DIR.parent / "hive"


def load_scenarios():
    with open(HIVE_WORKFLOW) as f:
        workflow = yaml.safe_load(f)
    return workflow["jobs"]["test"]["strategy"]["matrix"]["scenario"]


def get_filter(scenario):
    limit = scenario.get("limit", "")
    include = scenario.get("include", [])
    tests = "|".join(include) if include else ""

    if limit and tests:
        return f"{limit}/{tests}"
    elif limit:
        return limit
    elif tests:
        return f"/{tests}"
    return ""


def list_scenarios(scenarios):
    print("Available scenarios (from hive.yml):")
    for s in scenarios:
        sim = s["sim"]
        limit = s.get("limit", "")
        print(f"  {sim}\t{limit}" if limit else f"  {sim}")
    print()
    print("Usage: run-hive-local.py [--skip-build] [--all | <sim> [limit]]")


def find_scenario(scenarios, sim, limit_arg):
    for s in scenarios:
        if s["sim"] != sim:
            continue
        limit = s.get("limit", "")
        if limit_arg and limit != limit_arg:
            continue
        if not limit_arg and limit:
            continue
        return s
    return None


def run_scenario(sim, filter_str):
    print()
    print(f"==> Running: {sim}" + (f" (filter: {filter_str})" if filter_str else ""))

    os.chdir(HIVE_DIR)
    args = ["./hive", "--sim", sim, "--client", "bera-reth", "--sim.parallelism", "8"]
    if filter_str:
        args.extend(["--sim.limit", filter_str])

    start_time = time.time()

    # The hive process returns non-zero exit code when tests fail, even on expected
    # failures so we need to parse the JSON to check if failures are expected
    result = subprocess.run(args)

    # Find JSON files created after we started
    logs_dir = HIVE_DIR / "workspace/logs"
    json_files = [f for f in logs_dir.glob("*.json") if f.name != "hive.json" and f.stat().st_mtime > start_time]

    # If hive failed and no results generated, it crashed
    if result.returncode != 0 and not json_files:
        print(f"Hive crashed with exit code {result.returncode}")
        return False

    if not json_files:
        print("No JSON results found")
        return True

    # Get the newest json file (note: don't run this script in parallel!)
    json_file = max(json_files, key=lambda p: p.stat().st_mtime)
    print(f"Validating: {json_file.name}")

    hive_assets = BERA_RETH_DIR / ".github/assets/hive"
    result = subprocess.run(
        [
            "python3",
            str(hive_assets / "parse.py"),
            str(json_file),
            "--exclusion",
            str(hive_assets / "expected_failures.yaml"),
            "--ignored",
            str(hive_assets / "ignored_tests.yaml"),
        ]
    )
    return result.returncode == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--skip-build", action="store_true", help="Skip Docker build")
    parser.add_argument("--all", action="store_true", help="Run all scenarios")
    parser.add_argument("sim", nargs="?", help="Simulator to run")
    parser.add_argument("limit", nargs="?", help="Limit filter")
    args = parser.parse_args()

    scenarios = load_scenarios()

    # List scenarios mode
    if not args.sim and not args.all:
        list_scenarios(scenarios)
        return 0

    # Check prerequisites
    print("==> Checking prerequisites...")
    if not (HIVE_DIR / "hive.go").exists():
        print(f"Error: Hive not found at {HIVE_DIR}")
        print("Set HIVE_DIR or clone hive there")
        return 1

    # Build hive if the binary does not exst
    if not (HIVE_DIR / "hive").exists():
        print("==> Building hive...")
        subprocess.run(["go", "build", "-o", "hive", "."], cwd=HIVE_DIR, check=True)

    # Build Docker image
    if not args.skip_build:
        print("==> Building bera-reth Docker image...")
        subprocess.run(
            [
                "docker",
                "build",
                "-t",
                "ghcr.io/berachain/bera-reth:nightly",
                "-f",
                str(BERA_RETH_DIR / ".github/assets/hive/Dockerfile"),
                "--build-arg",
                "CARGO_BIN=bera-reth",
                "--build-arg",
                "BUILD_PROFILE=hivetests",
                str(BERA_RETH_DIR),
            ],
            check=True,
        )

    # Run scenarios
    failed = False
    if args.all:
        print("==> Running all scenarios...")
        for s in scenarios:
            filter_str = get_filter(s)
            if not run_scenario(s["sim"], filter_str):
                failed = True
    else:
        scenario = find_scenario(scenarios, args.sim, args.limit)
        if not scenario:
            print(f"Error: Scenario not found: {args.sim} {args.limit or ''}")
            return 1
        filter_str = get_filter(scenario)
        if not run_scenario(args.sim, filter_str):
            failed = True

    print()
    if failed:
        print("==> FAILED: Some scenarios had unexpected failures")
        return 1
    print("==> All scenarios passed!")
    return 0


if __name__ == "__main__":
    sys.exit(main())
