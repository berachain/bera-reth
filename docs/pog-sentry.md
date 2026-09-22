# `pog_sentry.py`

Cron collector in `scripts/pog_sentry.py`. One process, one canary, then exit.
Talks to one `bera-reth` IPC socket that already has `--bera.pog`. Holds the
signer; the node does not.

Namespace: [proof-of-gossip.md](proof-of-gossip.md).

## Run

Needs Python 3.10+, Foundry `cast` on `PATH`, a hex private-key file, and a
funded EOA.

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

`--dry-run` still opens IPC and reads the key. It picks a peer and writes
metrics; it does not sign or send.

```bash
POG_SENTRY_SELF_TEST=1 python3 scripts/pog_sentry.py
```

That path runs `_self_test()` and skips `--key-file`.

| Flag | Default | Script use |
|---|---|---|
| `--ipc` | `/tmp/reth.ipc` | `cast rpc --ipcpath` |
| `--key-file` | required | Hex key, `0x` optional. Empty file exits 1. |
| `--state-dir` | `./pog-sentry` | Created if missing. Holds `attempts.jsonl` and `metrics.prom`. |
| `--cooldown-secs` | `259200` | Seconds after `landed` or `timeout` before that `peerId` is eligible again. |
| `--strikes` | `3` | Distinct timeout hashes that skip-list a `peerId`. |
| `--health-window-secs` | `86400` | A `landed` row must be this recent or `pog_penalize` is held. |
| `--no-penalize` | off | Skip the penalize sweep. |
| `--dry-run` | off | Print `{pick, skipped}` JSON; no send. |

## Tick

`main()` does this, in order:

1. Read `--key-file`.
2. Call `pog_nodeStatus`. Missing object or IPC/`cast` failure exits 1. `syncing`
   writes metrics as `syncing` and exits 0.
3. Load `attempts.jsonl`. For every latest row still `sent`, call `pog_sends` and
   `eth_getTransactionReceipt`. Append a follow-up row (`reconciled: true`) if
   the hash has `landed` or `timeout`.
4. Unless `--no-penalize`, run `penalize_sweep`: every skip-listed `peerId` with
   no `penalized` row yet. If no `landed` row sits inside
   `--health-window-secs`, print a hold line to stderr and skip the RPC. Else
   call `pog_penalize` once per due id. Success appends `status=penalized`;
   RPC failure appends `penalize_error` and retries next tick.
5. If `pog_sends` has `status=sent` for this signer, print `signer already
   inflight on node`, outcome `idle`, exit 0.
6. `pog_peers`. Normalize `peerId` (lowercase, strip `0x`). `pick_peer` drops
   skip-listed ids and ids whose last `landed`/`timeout` is inside cooldown.
   Remaining sort never-tested first (`last_done_at` missing), then oldest
   `last_done_at`. Empty set: `no cold connected peer`, outcome `idle`.
7. `--dry-run` prints the pick and skipped set, outcome `idle`.
8. `eth_chainId`, `eth_getTransactionCount(from, latest)`, `cast mktx`: EIP-1559
   1-wei self-transfer, 21_000 gas, 1 gwei tip, 2 gwei max fee.
9. `pog_sendRawTransaction`. On RPC error, append `status=refused`, outcome
   `refused`, exit 0.
10. Append `status=sent`. Poll `pog_sends` every 2 seconds for up to 30 seconds
    until that hash is `landed` or `timeout`. If the deadline wins, record
    `timeout` locally. Print the final row as JSON. Exit 0.

## State files

`attempts.jsonl` is append-only. The latest row for a `txHash` wins. Send rows
include `ts`, `peerId`, `enode`, `direction`, `client`, `txHash`, `nonce`,
`from`, `status`, and optionally `blockNumber`, `error`, `reconciled`. Ban rows
are `ts`, `peerId`, `status`, optionally `connected` or `error`.

`status` values written by the script: `sent`, `landed`, `timeout`, `refused`,
`penalized`, `penalize_error`.

Skip list is derived: a `peerId` with `--strikes` distinct timeout hashes in
the latest-by-tx map. It is not a separate file.

`metrics.prom` is rewritten on every exit path that reaches `finish()`:

| Series | Source |
|---|---|
| `pog_sentry_checked_total` | Distinct `txHash` values in the log |
| `pog_sentry_first_day` | Peers whose first log `ts` is today UTC |
| `pog_sentry_skipped` | Size of the skip list |
| `pog_sentry_penalized_total` | Distinct `peerId`s with a `penalized` row |
| `pog_sentry_ban_pending` | Skip-listed peers held this tick by the health window |
| `pog_sentry_last_outcome_info{result=...}` | `1` on this process outcome, `0` on the rest |

`result` is `landed`, `timeout`, `idle`, `refused`, `syncing`, or `error`.

## Exit

| Code | Outcome |
|---|---|
| 0 | `landed`, `timeout`, `idle`, `syncing`, `refused` |
| 1 | Empty key file, IPC/`cast` failure, or unexpected send body (`error`) |
