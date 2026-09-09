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

## Related tooling

- `bera-reth db --datadir <dir> settings` — inspect the stored storage settings (layout
  version) of a datadir.
- ERA history import (`--era.enable`) exists upstream but there are no published
  Berachain ERA files yet, so it is not usable on Berachain today.
- The JIT EVM is not included in this build because reth's `jit` feature is disabled.
