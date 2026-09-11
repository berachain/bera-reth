#!/usr/bin/env python3
"""One-node Proof of Gossip sentry (Bronze++).

Sends at most one signed canary per process, then exits. Meant for cron against a
single bera-reth IPC socket with `--bera.pog`. The node never holds this key.

Needs
  - `cast` on PATH (Foundry)
  - Python 3.10+
  - a hex private key file (0x-prefixed or not)
  - IPC to a node that already has `pog_nodeStatus`

Run
  python3 scripts/pog_sentry.py --key-file /secret/pog.key \\
      --ipc /tmp/reth.ipc --state-dir /var/lib/pog-sentry

  */10 * * * * python3 /opt/bera-reth/scripts/pog_sentry.py \\
      --key-file /secret/pog.key --ipc /tmp/reth.ipc \\
      --state-dir /var/lib/pog-sentry

  python3 scripts/pog_sentry.py --key-file KEY --dry-run
      # pick a peer, write metrics, do not send

  POG_SENTRY_SELF_TEST=1 python3 scripts/pog_sentry.py
      # scheduler unit check, no node

State dir (created if missing)
  attempts.jsonl   append-only log; latest row per txHash wins
  metrics.prom     node_exporter textfile: checked, first-day, skipped, last outcome

Schedule
  Connected peers only. Never-tested first, then oldest completed test.
  Default cooldown is 3 days after landed or timeout (timeouts still count).
  Three distinct timeout hashes for one peerId → local skip list.

Ban
  A skip-listed peer is penalized once via pog_penalize, which drops it below
  Reth's ban threshold. Held back unless some canary landed inside the health
  window, so a chain-wide stall cannot walk the node off the network.

Canary
  EIP-1559 1 wei self-transfer, 21_000 gas, 1 gwei tip, 2 gwei max fee.
  Nonce is eth_getTransactionCount(latest). Aborts if that signer is inflight.

Exit
  0  send finished, idle, syncing, or refused
  1  RPC/cast/key failure
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

COOLDOWN_SECS = 3 * 24 * 60 * 60
STRIKES = 3
HEALTH_WINDOW_SECS = 24 * 60 * 60
POLL_SECS = 30
POLL_INTERVAL = 2
CANARY_WEI = 1
GAS_LIMIT = 21_000
PRIORITY_FEE_WEI = 1_000_000_000
MAX_FEE_WEI = 2_000_000_000


def load_attempts(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def append_attempt(path: Path, row: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as f:
        f.write(json.dumps(row, separators=(",", ":")) + "\n")


def latest_by_tx(attempts: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in attempts:
        h = row.get("txHash")
        if h:
            out[h] = row
    return out


def peer_stats(attempts: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    latest = latest_by_tx(attempts)
    stats: dict[str, dict[str, Any]] = defaultdict(
        lambda: {"timeouts": 0, "last_done_at": None, "first_at": None, "seen": False}
    )
    for row in attempts:
        pid = row.get("peerId")
        if not pid:
            continue
        st = stats[pid]
        st["seen"] = True
        ts = int(row.get("ts") or 0)
        if st["first_at"] is None or ts < st["first_at"]:
            st["first_at"] = ts
    for row in latest.values():
        pid = row.get("peerId")
        status = row.get("status")
        ts = int(row.get("ts") or 0)
        if not pid:
            continue
        st = stats[pid]
        if status == "timeout":
            st["timeouts"] += 1
        if status in ("landed", "timeout"):
            prev = st["last_done_at"]
            if prev is None or ts > prev:
                st["last_done_at"] = ts
    return stats


def skipped_peers(stats: dict[str, dict[str, Any]], strikes: int) -> set[str]:
    return {pid for pid, st in stats.items() if int(st["timeouts"]) >= strikes}


def penalized_peers(attempts: list[dict[str, Any]]) -> set[str]:
    return {r["peerId"] for r in attempts if r.get("status") == "penalized" and r.get("peerId")}


def landed_recently(attempts: list[dict[str, Any]], now: int, window_secs: int) -> bool:
    """Did anything land lately? Guards against banning the whole peer set."""
    for row in latest_by_tx(attempts).values():
        if row.get("status") == "landed" and now - int(row.get("ts") or 0) <= window_secs:
            return True
    return False


def penalize_sweep(
    ipc: str,
    log_path: Path,
    attempts: list[dict[str, Any]],
    stats: dict[str, dict[str, Any]],
    now: int,
    strikes: int,
    window_secs: int,
) -> tuple[list[dict[str, Any]], int]:
    """Ban skip-listed peers the node has not been told about yet."""
    due = sorted(skipped_peers(stats, strikes) - penalized_peers(attempts))
    if not due:
        return [], 0
    if not landed_recently(attempts, now, window_secs):
        print(f"no land inside health window; holding {len(due)} ban(s)", file=sys.stderr)
        return [], len(due)

    rows: list[dict[str, Any]] = []
    for pid in due:
        row: dict[str, Any] = {"ts": int(time.time()), "peerId": pid}
        try:
            resp = ipc_rpc(ipc, "pog_penalize", pid)
            row["status"] = "penalized"
            if isinstance(resp, dict):
                row["connected"] = bool(resp.get("connected"))
        except RuntimeError as e:
            row["status"] = "penalize_error"
            row["error"] = str(e)
            print(e, file=sys.stderr)
        append_attempt(log_path, row)
        rows.append(row)
    return rows, 0


def pick_peer(
    peers: list[dict[str, Any]],
    stats: dict[str, dict[str, Any]],
    now: int,
    cooldown_secs: int,
    strikes: int,
) -> dict[str, Any] | None:
    banned = skipped_peers(stats, strikes)
    eligible: list[tuple[int, dict[str, Any]]] = []
    for p in peers:
        pid = p["peerId"]
        if pid in banned:
            continue
        st = stats.get(pid)
        last = None if st is None else st["last_done_at"]
        if last is not None and now - int(last) < cooldown_secs:
            continue
        # Never-tested first (last is None), then oldest last_done_at.
        rank = 0 if last is None else int(last)
        eligible.append((rank, p))
    if not eligible:
        return None
    eligible.sort(key=lambda x: x[0])
    return eligible[0][1]


def first_day_count(stats: dict[str, dict[str, Any]], now: int) -> int:
    day = datetime.fromtimestamp(now, tz=timezone.utc).date()
    n = 0
    for st in stats.values():
        first = st.get("first_at")
        if first is None:
            continue
        if datetime.fromtimestamp(int(first), tz=timezone.utc).date() == day:
            n += 1
    return n


def write_metrics(
    path: Path,
    *,
    checked: int,
    first_day: int,
    skipped: int,
    penalized: int,
    ban_pending: int,
    last_outcome: str,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    body = (
        "# HELP pog_sentry_checked_total Canary attempts recorded by this sentry.\n"
        "# TYPE pog_sentry_checked_total counter\n"
        f"pog_sentry_checked_total {checked}\n"
        "# HELP pog_sentry_first_day Peers whose first attempt was today UTC.\n"
        "# TYPE pog_sentry_first_day gauge\n"
        f"pog_sentry_first_day {first_day}\n"
        "# HELP pog_sentry_skipped Peers at the timeout strike skip list.\n"
        "# TYPE pog_sentry_skipped gauge\n"
        f"pog_sentry_skipped {skipped}\n"
        "# HELP pog_sentry_penalized_total Peers this sentry sent to pog_penalize.\n"
        "# TYPE pog_sentry_penalized_total counter\n"
        f"pog_sentry_penalized_total {penalized}\n"
        "# HELP pog_sentry_ban_pending Skip-listed peers held back by the health window.\n"
        "# TYPE pog_sentry_ban_pending gauge\n"
        f"pog_sentry_ban_pending {ban_pending}\n"
        "# HELP pog_sentry_last_outcome_info Last run outcome (1 = this result).\n"
        "# TYPE pog_sentry_last_outcome_info gauge\n"
    )
    for result in ("landed", "timeout", "idle", "refused", "syncing", "error"):
        val = 1 if result == last_outcome else 0
        body += f'pog_sentry_last_outcome_info{{result="{result}"}} {val}\n'
    path.write_text(body)


def run_cast(args: list[str]) -> str:
    proc = subprocess.run(
        ["cast", *args],
        check=False,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        err = (proc.stderr or proc.stdout or "").strip()
        raise RuntimeError(f"cast {' '.join(args)} failed: {err}")
    return proc.stdout.strip()


def ipc_rpc(ipc: str, method: str, *params: Any) -> Any:
    args = ["rpc", "--ipcpath", ipc, method]
    for p in params:
        if isinstance(p, (dict, list)):
            args.append(json.dumps(p))
        else:
            args.append(str(p))
    out = run_cast(args)
    if not out:
        return None
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return out


def signer_address(key: str) -> str:
    return run_cast(["wallet", "address", "--private-key", key])


def sign_canary(key: str, to: str, nonce: int, chain_id: int) -> str:
    return run_cast(
        [
            "mktx",
            "--private-key",
            key,
            "--nonce",
            str(nonce),
            "--value",
            str(CANARY_WEI),
            "--gas-limit",
            str(GAS_LIMIT),
            "--priority-gas-price",
            str(PRIORITY_FEE_WEI),
            "--gas-price",
            str(MAX_FEE_WEI),
            "--chain",
            str(chain_id),
            to,
        ]
    )


def parse_qty(v: Any) -> int:
    if isinstance(v, int):
        return v
    s = str(v)
    if s.startswith(("0x", "0X")):
        return int(s, 16)
    return int(s)


def normalize_peer_id(pid: str) -> str:
    return pid.lower().removeprefix("0x")


def reconcile(
    attempts: list[dict[str, Any]],
    ipc: str,
    log_path: Path,
) -> list[dict[str, Any]]:
    latest = latest_by_tx(attempts)
    sends = ipc_rpc(ipc, "pog_sends") or []
    by_hash = {}
    for s in sends:
        h = str(s.get("txHash", ""))
        if h.startswith("0x"):
            by_hash[h.lower()] = s
        else:
            by_hash["0x" + h.lower()] = s

    extra: list[dict[str, Any]] = []
    for h, row in latest.items():
        if row.get("status") != "sent":
            continue
        key = h if h.startswith("0x") else "0x" + h
        key = key.lower()
        remote = by_hash.get(key)
        receipt = ipc_rpc(ipc, "eth_getTransactionReceipt", key)
        new_status = None
        block_number = None
        if isinstance(receipt, dict) and receipt.get("blockNumber"):
            new_status = "landed"
            block_number = int(receipt["blockNumber"], 16)
        elif isinstance(remote, dict) and remote.get("status") in ("landed", "timeout"):
            new_status = remote["status"]
            bn = remote.get("blockNumber")
            block_number = bn
        if new_status is None:
            continue
        update = dict(row)
        update["status"] = new_status
        update["blockNumber"] = block_number
        update["ts"] = int(time.time())
        update["reconciled"] = True
        append_attempt(log_path, update)
        extra.append(update)
    return attempts + extra


def poll_send(ipc: str, tx_hash: str, deadline: float) -> dict[str, Any] | None:
    want = tx_hash.lower()
    while time.time() < deadline:
        sends = ipc_rpc(ipc, "pog_sends") or []
        for s in sends:
            h = str(s.get("txHash", "")).lower()
            if h == want or h == want.removeprefix("0x") or ("0x" + h) == want:
                if s.get("status") in ("landed", "timeout"):
                    return s
        time.sleep(POLL_INTERVAL)
    return None


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description="Send one PoG canary to a cold connected peer, then exit.",
        epilog=(
            "examples:\n"
            "  %(prog)s --key-file /secret/pog.key --ipc /tmp/reth.ipc\n"
            "  %(prog)s --key-file /secret/pog.key --dry-run\n"
            "  POG_SENTRY_SELF_TEST=1 %(prog)s\n"
            "\n"
            "Runbook: docs/pog-sentry.md\n"
            "IPC contract: docs/proof-of-gossip.md"
        ),
    )
    p.add_argument(
        "--ipc",
        default="/tmp/reth.ipc",
        metavar="PATH",
        help="reth JSON-RPC IPC socket (default: %(default)s)",
    )
    p.add_argument(
        "--key-file",
        required=True,
        metavar="PATH",
        help="hex private key for the canary signer (file mode 0600 recommended)",
    )
    p.add_argument(
        "--state-dir",
        default="./pog-sentry",
        metavar="DIR",
        help="attempts.jsonl and metrics.prom (default: %(default)s)",
    )
    p.add_argument(
        "--cooldown-secs",
        type=int,
        default=COOLDOWN_SECS,
        metavar="N",
        help="seconds before retesting a peer after land/timeout (default: %(default)s = 3d)",
    )
    p.add_argument(
        "--strikes",
        type=int,
        default=STRIKES,
        metavar="N",
        help="timeout hashes before skip-listing a peerId (default: %(default)s)",
    )
    p.add_argument(
        "--health-window-secs",
        type=int,
        default=HEALTH_WINDOW_SECS,
        metavar="N",
        help="a canary must have landed this recently to allow a ban (default: %(default)s = 1d)",
    )
    p.add_argument(
        "--no-penalize",
        action="store_true",
        help="never call pog_penalize; keep the skip list local to this sentry",
    )
    p.add_argument(
        "--dry-run",
        action="store_true",
        help="print the chosen peer and skipped set; do not sign or send",
    )
    return p.parse_args()


def main() -> int:
    args = parse_args()
    state = Path(args.state_dir)
    log_path = state / "attempts.jsonl"
    metrics_path = state / "metrics.prom"
    now = int(time.time())

    ban_pending = 0

    def finish(outcome: str, attempts: list[dict[str, Any]]) -> int:
        stats = peer_stats(attempts)
        write_metrics(
            metrics_path,
            checked=len({r.get("txHash") for r in attempts if r.get("txHash")}),
            first_day=first_day_count(stats, now),
            skipped=len(skipped_peers(stats, args.strikes)),
            penalized=len(penalized_peers(attempts)),
            ban_pending=ban_pending,
            last_outcome=outcome,
        )
        return 0 if outcome not in ("error",) else 1

    key = Path(args.key_file).read_text().strip()
    if not key:
        print("key file is empty", file=sys.stderr)
        return 1

    try:
        status = ipc_rpc(args.ipc, "pog_nodeStatus")
    except RuntimeError as e:
        print(e, file=sys.stderr)
        return finish("error", load_attempts(log_path))

    if not isinstance(status, dict):
        print("pog_nodeStatus returned no object; is --bera.pog enabled?", file=sys.stderr)
        return finish("error", load_attempts(log_path))
    if status.get("syncing"):
        print("node is syncing")
        return finish("syncing", load_attempts(log_path))

    attempts = reconcile(load_attempts(log_path), args.ipc, log_path)

    if not args.no_penalize:
        banned_rows, ban_pending = penalize_sweep(
            args.ipc,
            log_path,
            attempts,
            peer_stats(attempts),
            now,
            args.strikes,
            args.health_window_secs,
        )
        attempts += banned_rows

    from_addr = signer_address(key)
    sends = ipc_rpc(args.ipc, "pog_sends") or []
    for s in sends:
        if str(s.get("from", "")).lower() == from_addr.lower() and s.get("status") == "sent":
            print("signer already inflight on node")
            return finish("idle", attempts)

    peers = ipc_rpc(args.ipc, "pog_peers") or []
    for p in peers:
        p["peerId"] = normalize_peer_id(p["peerId"])
    stats = peer_stats(attempts)
    target = pick_peer(peers, stats, now, args.cooldown_secs, args.strikes)
    if target is None:
        print("no cold connected peer")
        return finish("idle", attempts)

    if args.dry_run:
        print(json.dumps({"pick": target, "skipped": sorted(skipped_peers(stats, args.strikes))}))
        return finish("idle", attempts)

    chain_id = parse_qty(ipc_rpc(args.ipc, "eth_chainId"))
    nonce = parse_qty(ipc_rpc(args.ipc, "eth_getTransactionCount", from_addr, "latest"))
    raw = sign_canary(key, from_addr, nonce, chain_id)
    try:
        sent = ipc_rpc(args.ipc, "pog_sendRawTransaction", target["peerId"], raw)
    except RuntimeError as e:
        print(e, file=sys.stderr)
        row = {
            "ts": now,
            "peerId": target["peerId"],
            "enode": target.get("enode"),
            "direction": target.get("direction"),
            "client": target.get("client"),
            "status": "refused",
            "error": str(e),
        }
        append_attempt(log_path, row)
        return finish("refused", attempts + [row])

    if not isinstance(sent, dict):
        print("unexpected send response", sent, file=sys.stderr)
        return finish("error", attempts)

    tx_hash = sent["txHash"]
    if isinstance(tx_hash, str) and not tx_hash.startswith("0x"):
        tx_hash = "0x" + tx_hash
    row = {
        "ts": now,
        "peerId": normalize_peer_id(sent.get("peerId", target["peerId"])),
        "enode": sent.get("enode") or target.get("enode"),
        "direction": target.get("direction"),
        "client": target.get("client"),
        "txHash": tx_hash,
        "nonce": nonce,
        "from": from_addr,
        "status": "sent",
    }
    append_attempt(log_path, row)

    polled = poll_send(args.ipc, tx_hash, time.time() + POLL_SECS)
    if polled is None:
        outcome = "timeout"
        row_done = dict(row)
        row_done["status"] = "timeout"
        row_done["ts"] = int(time.time())
    else:
        outcome = polled.get("status", "timeout")
        row_done = dict(row)
        row_done["status"] = outcome
        row_done["blockNumber"] = polled.get("blockNumber")
        row_done["ts"] = int(time.time())
    append_attempt(log_path, row_done)
    print(json.dumps(row_done))
    final = outcome if outcome in ("landed", "timeout") else "error"
    return finish(final, attempts + [row, row_done])


def _self_test() -> None:
    now = 1_000_000
    peers = [
        {"peerId": "aa", "enode": "enode://aa@1.1.1.1:1"},
        {"peerId": "bb", "enode": "enode://bb@1.1.1.2:1"},
        {"peerId": "cc", "enode": "enode://cc@1.1.1.3:1"},
    ]
    attempts = [
        {"ts": now - 10, "peerId": "aa", "txHash": "0x1", "status": "landed"},
        {"ts": now - 10, "peerId": "cc", "txHash": "0x2", "status": "timeout"},
        {"ts": now - 11, "peerId": "cc", "txHash": "0x3", "status": "timeout"},
        {"ts": now - 12, "peerId": "cc", "txHash": "0x4", "status": "timeout"},
    ]
    stats = peer_stats(attempts)
    assert "cc" in skipped_peers(stats, 3)
    pick = pick_peer(peers, stats, now, COOLDOWN_SECS, 3)
    assert pick is not None and pick["peerId"] == "bb"
    pick2 = pick_peer(peers, stats, now + COOLDOWN_SECS + 1, COOLDOWN_SECS, 3)
    assert pick2 is not None and pick2["peerId"] in ("aa", "bb")

    assert landed_recently(attempts, now, HEALTH_WINDOW_SECS)
    assert not landed_recently(attempts, now + HEALTH_WINDOW_SECS + 1, HEALTH_WINDOW_SECS)

    def sweep(rows: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], int]:
        return penalize_sweep(
            "/nonexistent", Path(os.devnull), rows, peer_stats(rows), now, 3, HEALTH_WINDOW_SECS
        )

    stalled = [r for r in attempts if r["peerId"] != "aa"]
    held, pending = sweep(stalled)
    assert held == [] and pending == 1, (held, pending)

    penalized = attempts + [{"ts": now, "peerId": "cc", "status": "penalized"}]
    assert penalized_peers(penalized) == {"cc"}
    again, pending = sweep(penalized)
    assert again == [] and pending == 0, (again, pending)
    print("self-test ok")


if __name__ == "__main__":
    if os.environ.get("POG_SENTRY_SELF_TEST") == "1":
        _self_test()
        sys.exit(0)
    try:
        sys.exit(main())
    except BrokenPipeError:
        sys.exit(0)
