# Proof of Gossip sentry

`scripts/pog_sentry.py` is a cron collector for one `bera-reth` node with
`--bera.pog`. Each process signs at most one canary, writes state, and exits. The
hex key stays on the collector host.

Node IPC contract: [proof-of-gossip.md](proof-of-gossip.md). Client duties:
[proof-of-gossip-client.md](proof-of-gossip-client.md).

## Requirements

- Python 3.10+
- Foundry `cast` on `PATH`
- IPC to a node that answers `pog_nodeStatus` (default `/tmp/reth.ipc`)
- Funded EOA whose key is in a file (`0600` recommended; `0x` prefix optional)
- Persistent `--state-dir` (default `./pog-sentry`)

The canary is an EIP-1559 1-wei self-transfer: 21_000 gas, 1 gwei tip, 2 gwei max
fee. The tip must clear the pool floor.

```bash
python3 scripts/pog_sentry.py \
  --key-file /secret/pog.key \
  --ipc /tmp/reth.ipc \
  --state-dir /var/lib/pog-sentry
```

```cron
*/10 * * * * python3 /opt/bera-reth/scripts/pog_sentry.py \
  --key-file /secret/pog.key --ipc /tmp/reth.ipc --state-dir /var/lib/pog-sentry
```

`--dry-run` still needs a live IPC socket and a key file. It picks a peer and
writes metrics; it does not sign or send.

```bash
POG_SENTRY_SELF_TEST=1 python3 scripts/pog_sentry.py
```

That path runs the scheduler unit check and skips `--key-file`.

| Flag | Default | Role |
|---|---|---|
| `--ipc` | `/tmp/reth.ipc` | JSON-RPC socket |
| `--key-file` | required | Canary signer |
| `--state-dir` | `./pog-sentry` | `attempts.jsonl` and `metrics.prom` |
| `--cooldown-secs` | `259200` (3 days) | Wait after `landed` or `timeout` before retesting that `peerId` |
| `--strikes` | `3` | Distinct timeout hashes that skip-list a `peerId` |
| `--health-window-secs` | `86400` (1 day) | A canary must have landed this recently before any ban goes out |
| `--no-penalize` | off | Measure only; never call `pog_penalize` |
| `--dry-run` | off | Choose peer, do not send |

## Schedule

Only currently connected peers from `pog_peers` are candidates. Skip-listed ids
are dropped. A peer with a completed test (`landed` or `timeout`) inside the
cooldown window is dropped. Remaining peers sort never-tested first, then oldest
`last_done_at`.

Timeouts count as completed tests, so a black hole waits a full cooldown before
another hash. Three timeout hashes for one `peerId` put it on a skip list derived
from the JSONL.

The node allows only one inflight send per recovered signer. The sentry aborts
with outcome `idle` if `pog_sends` already has `status=sent` for this `from`.
Parallel probes need more funded accounts, not a second process on the same key.

## Banning a useless peer

A peer that reaches the strike count is a peer we handed transactions to three
separate times and never saw one land. The sweep runs before the send, on every
tick, so a ban goes out even when this tick has nothing to probe.

Each skip-listed `peerId` gets one `pog_penalize` call. The node applies
`ReputationChangeKind::BadProtocol`, which puts the peer under Reth's ban
threshold: disconnect now, no redial for `ban_duration` (12 hours by default,
`--peers.ban-duration`). Trusted peers are exempt. The sentry's own skip list
outlives the ban, so a peer that reconnects is never probed again.

The call is recorded as a `penalized` row and never repeats for that `peerId`.
A failed call lands as `penalize_error` and retries next tick.

**The health guard.** No ban goes out unless some canary landed within
`--health-window-secs`. Without it, a raised tip floor or a stalled chain times
out every probe, and strike counting would walk the node off the entire network
one peer at a time. Held bans are counted in `pog_sentry_ban_pending` and go out
on the first tick after something lands. Alert on that gauge staying above zero.

Run with `--no-penalize` while establishing a baseline on a new node.

## State

`--state-dir` is created if missing.

`attempts.jsonl` is append-only. The latest row for a `txHash` wins. Typical
fields: `ts`, `peerId`, `enode`, `direction`, `client`, `txHash`, `nonce`,
`from`, `status`, optional `blockNumber`, `error`, `reconciled`, `connected`.

`status` is `sent`, `landed`, `timeout`, `refused`, `penalized`, or
`penalize_error`. Ban rows carry `ts`, `peerId`, and `status` only.

On start, every latest row still `sent` is checked against `pog_sends` and
`eth_getTransactionReceipt`. A land or node timeout is appended (`reconciled:
true`) so a killed job does not lose the nonce. A row that is still `sent` on
both sides leaves the signer inflight; this run then idles.

A `timeout` from the 30-second poll can still land later. Reconcile on the next
start is what flips it. Do not bump nonce from `timeout` alone.

`metrics.prom` is a node_exporter textfile. Point `--collector.textfile.directory`
at `--state-dir` or copy the file there after each run.

| Series | Meaning |
|---|---|
| `pog_sentry_checked_total` | Distinct `txHash` values in the log |
| `pog_sentry_first_day` | Peers whose first log row is today UTC |
| `pog_sentry_skipped` | `peerId`s at the strike skip list |
| `pog_sentry_penalized_total` | `peerId`s this sentry has sent to `pog_penalize` |
| `pog_sentry_ban_pending` | Skip-listed peers held back by the health window |
| `pog_sentry_last_outcome_info{result=...}` | `1` on the outcome of this process, `0` on the rest |

`result` is one of `landed`, `timeout`, `idle`, `refused`, `syncing`, `error`.

## Exit codes

| Code | When |
|---|---|
| 0 | Send finished (`landed` / `timeout`), or idle / syncing / refused |
| 1 | IPC, `cast`, or empty key file |

Stdout on a completed send is one JSON object (the final attempt row). Refused
sends print the RPC error on stderr and append `status=refused`.

The poll waits up to 30 seconds (`2s` interval). The node marks a send `timeout`
after 25 seconds, so a live row usually resolves inside one process. If the poll
deadline wins first, the sentry records `timeout` locally; reconcile can still
see a later `landed`.

## Not in this script

First-hear provenance, a second signer, `/24` quotas, and more than one send per
process.
