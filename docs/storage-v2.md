# Storage V2 Operator Guide

Bera-reth is built on reth `v2.5.2`, which ships reth's new hot/cold storage layout
("Storage V2"). This guide covers what changed and what, if anything, node operators
need to do.

## What changed

The V1 layout stored everything in a single MDBX database. V2 splits storage by access
pattern:

- **RocksDB** — history indices and transaction-hash lookups.
- **Static files** — historical account and storage changesets, receipts.
- **MDBX** — hashed state and everything else, as before.

Upstream reports roughly 20–30% smaller full-node datadirs and much faster persistence.
Details: [reth.rs/run/storage](https://reth.rs/run/storage/).

The layout of a datadir is recorded in its database metadata and always takes precedence:
upgrading the bera-reth binary never converts a datadir, and `--storage.v2` only selects
the layout when a **new** datadir is created.

## What operators need to do

| Situation | Action |
|---|---|
| Fresh node (new datadir) | Nothing — V2 is the default for new datadirs. |
| Existing node, staying on V1 | Nothing — V1 datadirs keep working on this binary. Note that upstream reth plans to drop V1 support in a future release, so plan a migration window. |
| Existing node, moving to V2 | Stop the node, run `bera-reth db migrate-v2` (below), restart. Or restore a V2 snapshot into a fresh datadir, or resync from scratch. |

Berachain publishes V2-layout [snapshots](https://snapshots.berachain.com/) for mainnet
and bepolia in pruned and archive flavors. `bera-reth download --chain <mainnet|bepolia>`
resolves the manifest for the chain automatically; pick the component set with
`--archive`, `--full`, or `--minimal` (or `--list` to inspect what is available).

## In-place migration

Stop the node first, then:

```bash
bera-reth db --datadir <datadir> migrate-v2 --chain <genesis.json|mainnet|bepolia>
```

The command moves changesets and receipts into static files, history indices and
transaction lookups into RocksDB, flips the stored layout to V2, and compacts the
remaining MDBX database. On the next start the pipeline rebuilds anything recomputable.

Plan for free disk space of at least the current datadir size while the migration runs
(new files are written alongside the old database before compaction); the final
footprint should end up smaller than V1. Upstream's Ethereum mainnet figures are
~30% savings for a full node — no Berachain-specific measurements are published yet.

### The migration must not be interrupted

`migrate-v2` is **not resumable**. It flips the stored layout to V2 partway through, then
clears the tables it has migrated and resets the affected stage checkpoints. If the
process is killed between those steps (OOM kill, a dropped `kubectl exec`/SSH session,
`SIGKILL`), the datadir is left half-converted: the layout says V2 but some tables still
hold V1-encoded data and the checkpoints still point at the old tip. In that state every
subsequent open — `migrate-v2` again, any `db` subcommand, or `node` — fails in the
storage consistency check before it can repair anything (typically a panic decoding trie
keys, or "Cannot unwind … beyond the AccountHistory limit" on pruned nodes). The only
recovery is to restore the V1 datadir from a backup or snapshot and run the migration
again, so:

- Run it detached from any interactive session (a Kubernetes Job or init container,
  `systemd-run`, `nohup … &` with logs to a file), never from a shell you might lose.
- Give the process the same memory limit the node runs with; the changeset and receipt
  passes hold sizeable write batches.
- Take a backup or note the snapshot you can restore from before starting.

The migration itself is usually quick relative to the rebuild that follows (a few
minutes on a mainnet full node); most of the wall-clock cost is on the first start.

### What to expect on the first start after migration

Start the node with **exactly the same flags as before**. No storage flag is needed: the
datadir's stored layout (now V2) always takes precedence over `--storage.v2`, and the
pruning mode is read from the datadir's `reth.toml`, so a `--full`/`--minimal` node stays
in that mode. Changing flags at this point is possible but unrelated to the migration.

On that first start the pipeline rebuilds everything `migrate-v2` deliberately dropped
instead of converting: the state trie (`AccountsTrie`/`StoragesTrie`, re-encoded in the
V2 key format), transaction senders, and any V1-side history indices that were not moved
to RocksDB. In the logs this looks like a normal staged sync, with these stages
dominating:

- `SenderRecovery` from `checkpoint=0` — recovers every sender for the whole chain;
  fast, minutes.
- `MerkleExecute` from `checkpoint=0` — walks the entire hashed state and recomputes the
  trie. This is the long one: roughly 1–2 hours on a mainnet full node, and it is the
  most memory-hungry part of the process. `checkpoint=0` does **not** advance during the
  stage; progress is shown by `stage_progress=NN%` on each `Committed stage progress`
  line (commits every ~10–20 s), and the ETA field swings widely between batches — trust
  the percentage, not the ETA. The stage ends with `Finished stage … MerkleExecute` once
  the computed root matches the header at the target block.
- `TransactionLookup`, `IndexAccountHistory`, `IndexStorageHistory`, `Finish` — quick,
  since their data was migrated into RocksDB rather than dropped.

Until the pipeline reaches `Finish` the node is syncing from the consensus layer's point
of view: `eth_syncing` reports true, Engine API calls are answered with `SYNCING`, and
the RPC serves only whatever state the stages have completed. Beacon-kit will simply wait.
Unlike `migrate-v2`, this pipeline **is** resumable — `MerkleExecute` checkpoints its
intra-stage progress — so a restart during the rebuild only costs the current batch, but
avoid it if you can. The one genuine failure mode here is a state-root mismatch at the end
of `MerkleExecute`; that would indicate the migrated hashed state is inconsistent and
would require restoring and redoing the migration.

Afterwards the node behaves as any V2 node; `bera-reth db --datadir <dir> settings get`
should report `storage_v2: true`.

## Opting a new datadir back into V1

```bash
bera-reth node --storage.v2=false ...
```

This only affects datadir creation; it never converts an existing database. Since V1 is
slated for removal upstream, treat it strictly as an escape hatch.

## Node modes for RPC operators

Archive remains the default. Two pruned presets exist for lighter RPC nodes:

- `--full` — keeps recent state plus a bounded history window.
- `--minimal` — maximum pruning, smallest disk footprint.

These are `bera-reth node` flags (and, with the same meaning, `bera-reth download`
component selectors). `db migrate-v2` does not accept them: it takes only the shared
`--datadir`/`--chain`/`--config` arguments and reads pruning from the datadir's
`reth.toml`, which the node writes on start whenever `--full`/`--minimal` change the
prune configuration. So a node that has been running with a preset is migrated with that
preset's pruning honored (migration starts each segment at its prune checkpoint, and with
receipt log-filter pruning receipts are left in MDBX), and a node's mode is chosen or
changed by (re)starting `node` with the flag, not during migration.

Pruning is destructive and irreversible. A pruned node can be bootstrapped from a pruned
Berachain snapshot (`download --full` / `--minimal`) instead of syncing the full history
from P2P.

### Pruned nodes: do not distance-prune sender recovery on V2

**If your node prunes `sender_recovery` with a `distance` (or `before`) mode, remove that
setting before the datadir becomes V2.** This applies to any custom pruning configuration —
the `--prune.senderrecovery.distance`/`--prune.senderrecovery.before` flags or the
`sender_recovery = { distance = … }` entry under `[prune.segments]` in `reth.toml`. The
`--full` and `--minimal` presets are unaffected (they use `sender_recovery = "full"`),
and archive nodes are unaffected (they don't prune).

Why: on V1 senders live in an MDBX table, where gaps are fine. On V2 they live in an
append-only static file whose transaction numbers must be contiguous. reth's
`SenderRecovery` stage, when given a distance, skips straight to `tip − distance` whenever
it has more than `distance` blocks to catch up on. On V2 that skip leaves a hole in the
static file and the stage fails on the very next append:

```
Stage encountered a fatal error: database integrity error occurred: trying to append row to
TransactionSenders at index #N but expected index #M stage=SenderRecovery
```

The node exits and every restart fails the same way until the configuration changes. It is
triggered by any catch-up larger than the distance — the first start after `migrate-v2`
or after restoring a snapshot that is more than `distance` blocks behind the network, and
later by any outage longer than that (with the common `distance = 10064` that is ~5.5 h
on mainnet). This is an upstream reth defect (the same class as reth
[#23463](https://github.com/paradigmxyz/reth/issues/23463), which fixed the `full` mode
but not `distance`).

What to change: drop the sender-recovery prune setting entirely. Senders are then kept for
every block, which costs about 20 bytes per transaction (low single-digit MB per day on
mainnet) and has no RPC impact. Alternatively `sender_recovery = "full"`
(`--prune.senderrecovery.full`) also works, but it makes `eth_getBlockBy*` with full
transactions recover senders on the fly. The other `distance` segments
(`transaction_lookup`, `receipts`, `account_history`, `storage_history`) are unaffected and
can stay as they are.

Removing the setting is safe for nodes still on V1, so there is no need to coordinate the
config change with the layout change: the pruner simply stops deleting senders and the
stage stops skipping. Berachain's own Helm charts have dropped the setting fleet-wide for
this reason.

How this applies to the two upgrade paths:

1. **Manual `migrate-v2` of an existing pruned node.** Change the prune configuration
   (remove the CLI flag and/or the `reth.toml` entry) *before* the first `node` start after
   migration. Because the node rewrites `[prune]` in `reth.toml` from its CLI flags on
   every start, removing only the TOML line while the CLI flag is still passed does
   nothing — remove the flag. If you already hit the error above, the datadir is intact:
   fix the configuration and start the node again; the stage resumes from its checkpoint
   and appends contiguously.
2. **Restoring a V2 snapshot and continuing.** The Berachain pruned snapshots are produced
   by nodes without sender-recovery pruning and ship a complete `TransactionSenders`
   static file up to the snapshot block. Start the restored node without the
   sender-recovery prune setting from the very first start: the snapshot is always some
   blocks behind the network, and if that gap exceeds your distance the stage skips ahead
   and fails immediately. Nothing else about the restore changes — your other prune flags
   and the `reth.toml` you carry over are fine.

## Related tooling

- `bera-reth db --datadir <dir> settings` — inspect the stored storage settings (layout
  version) of a datadir.
- ERA history import (`--era.enable`) exists upstream but there are no published
  Berachain ERA files yet, so it is not usable on Berachain today.
- The JIT EVM is not included in this build because reth's `jit` feature is disabled.
