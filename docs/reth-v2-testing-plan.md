# Reth v2.5.0 / Storage V2 — Network Testing Plan

Validation plan for the reth v1.11.4 → v2.5.0 (Storage V2) upgrade beyond the automated
suite: devnets with a custom validator set that mimic mainnet, mixed-version and
mixed-storage networks, sync/pruning matrices, disaster-recovery drills, and the
v2.5.0-specific gotchas. Companion to `docs/storage-v2.md` (operator guide).

## 1. What is already covered (don't re-test first)

- **178 automated tests** on the branch: payload-ID vectors (gas-limit inclusion +
  legacy-ID stability when absent), golden `Compact` byte vectors (post-Prague1 and
  pre-Prague1 headers, PoL envelope), seven Storage V2 e2e tests (fresh-V2 default,
  `--storage.v2=false` escape hatch, V1 readability, in-place migration, post-migration
  queryability of historical state/receipts/tx lookups, pruned-receipt movement,
  idempotency), engine-defaults regression test, EIP-7685 requests preservation,
  `getPayloadV4P11` post-Osaka, blob/deposit/PoL e2e.
- **Single-node in-place migrations** already validated on bepolia and mainnet datadirs;
  migrated nodes resumed syncing normally.
- **Local two-client pairing** (`scripts/test-block-progression.sh`) green against
  beacon-kit `main`.

Everything below targets what the automated suite cannot see: multi-node consensus,
mixed fleets, real datadir scale, and operational procedures.

## 2. Top risks driving this plan

1. **Consensus divergence** between old-binary and new-binary validators, or between
   V1/V2/migrated storage — a chain split on a single-slot-finality chain.
2. **Irreversibility**: a migrated V2 datadir cannot run on the old binary. The only
   downgrade paths are backup restore or resync.
3. **`beacond rollback` interplay with v2.5.0's `-38006`**: reth now rejects
   forkchoice updates below the EL-finalized block. Beacon-kit finalizes every height,
   so the CL rollback runbook may no longer work unchanged against a v2.5.0 EL.
4. **PoL system-call gas observability**: the 30M gas pin must hold — the EIP-8037
   reservoir in revm 42 would otherwise change `gasleft()` seen by the PoL distributor,
   which is consensus-breaking. Do not conflate the two gas numbers: the PoL
   system-call/tx budget is intentionally pinned at 30M (state compatibility), while
   the latest Berachain networks run a **36M block gas limit**.
5. **Payload cache vs gas-limit steering**: payload IDs now include `target_gas_limit`
   precisely so a cached payload can't be served for a stale limit — behavior only
   observable with a live CL driving limit changes.
6. **Pruned-node checkpoint handling**: v2.5.0 contains multiple fixes in this area
   (crash loops, checkpoint regressions) — treat pruned configs as a hot zone.

## 3. Environments and node matrix

| Environment | Purpose |
|---|---|
| Local multi-node devnet (docker/kurtosis, 4–7 validators) | Fast iteration on consensus scenarios, kill/restart drills |
| Dedicated devnet with custom validator set (mainnet-test cluster) | Mainnet rehearsal: mainnet-clone chain spec, realistic validator count and stake distribution, full fork ladder (Prague1–4, Osaka1) active, mainnet-like block times and the current **36M block gas limit** (verify the devnet genesis/CL target actually carries 36M — the stock local beacon-kit dev genesis uses a lower limit) |
| Bepolia canaries | Long soak on a real public network (non-validator first, then validators) |
| Mainnet canary full nodes | Final pre-rollout observation; never validators first |

Axes to combine (each scenario below names the cells it needs):

- **EL binary**: old (bera-reth v1.4.x / reth 1.11.4) vs new (v1.5.0 / reth 2.5.0).
- **Storage**: V1 (un-migrated on new binary), V2-fresh (resynced), V2-migrated.
- **Role**: validator, full node (`--full`), archive (default), `--minimal`, custom
  `prune.toml` (e.g. receipts log-filter, distance-based history).
- **CL**: beacon-kit versions in the supported pairing window (≥ v1.4.1 treats EL HTTP
  4xx as fatal; nightly with the migrate tooling from beacon-kit #3156).

## 4. Consensus-critical scenarios (custom validator set)

Global pass criteria for every scenario in this section: a single canonical fork across
the fleet; zero `newPayload → INVALID` responses; identical `stateRoot` (sampled via
`eth_getBlockByNumber` across all nodes at the same heights); no validator falls out of
consensus (CometBFT app hash = beacon state root, so EL divergence surfaces immediately).

| ID | Scenario | Key steps | Extra pass criteria |
|----|----------|-----------|---------------------|
| CV-1 | New-version baseline | All validators v1.5.0 + fresh V2, soak under load (§8) | Round counts and missed heights at parity with the v1.4.x baseline |
| CV-2 | Mixed binaries (rolling-upgrade rehearsal) | 20/80, 50/50, 80/20 old:new splits; run long enough for several full proposer rotations | Blocks proposed by each flavor accepted by all others; no round escalation attributable to one flavor |
| CV-3 | Mixed storage | New binary fleet: some validators V1 un-migrated, some V2-fresh, some V2-migrated | Same as global; no performance cliff for the V1 nodes |
| CV-4 | Incremental validator migration (the mainnet rehearsal) | Under continuous load, one validator at a time: stop → `bera-reth db migrate-v2` → restart → wait for full catch-up + a successful proposal before the next one; always keep ≥ 2/3 voting power online | Network liveness throughout; per-validator downtime recorded (drives mainnet maintenance-window comms); repeat once with a deliberately failed migration recovered from backup |
| CV-5 | PoL / system-call parity | PoL distributor runs every block by construction; add blocks that exercise `gasleft()`-sensitive paths; compare receipts, logs, and gas used across old/new validators | Receipts root and state root byte-identical old vs new for the same blocks; 30M system-call budget confirmed (no EIP-8037 reservoir visible) |
| CV-6 | Gas-limit steering | Drive a target-gas-limit change from beacon-kit on new-EL proposers around the production value (e.g. 36M → 40M and 36M → 30M); include a change between rounds at the same height | Built block honors the fresh limit — never a cached payload with the stale limit; old-EL validators accept the block; distinct payload IDs observed for distinct limits |
| CV-7 | Deposits / execution requests | Continuous deposit txs plus bursts; verify EIP-6110 requests present in payload envelopes and processed by beacon-kit | No dropped requests (regression guard for the built-payload conversion fix); deposit store on CL matches EL logs |
| CV-8 | Blobs | EIP-4844 traffic at target and max blobs; mixed-version fleet | Sidecar propagation and KZG validation clean on all flavors; pool accepts/serves blob txs (v2.5.0 blob-cell availability refactor) |
| CV-9 | Multi-round stress | Kill proposers mid-build; inject network latency (tc/netem); force same-height re-proposals with new timestamps | Repeated FCU + `getPayload` for the same height across rounds returns fresh, valid payloads on both EL versions |
| CV-10 | Engine defaults with bare flags | Run validators with **no** `--engine.*` flags | Effective `persistence-threshold=0` and `memory-block-buffer-target=0` (pinned defaults); after `kill -9` a validator EL restarts exactly at the finalized tip with no replay gap |
| CV-11 | Engine API surface | Watch beacon-kit ↔ EL traffic across the fleet | `getPayloadV4P11` served post-Osaka; pre-Osaka `getPayloadV5` returns JSON-RPC `UnsupportedFork` (-38005) and never an HTTP 4xx/5xx (fatal to beacon-kit ≥ 1.4.1); `engine_exchangeCapabilities` lists what beacon-kit expects |
| CV-12 | Fork-boundary crossing | On the devnet clone, schedule the next upcoming fork in the future and cross it live with the new fleet (and mixed fleet if both binaries support the fork) | Clean activation; no divergence at the boundary block |

## 5. Sync, storage, and pruning matrix

| ID | Scenario | Key steps | Pass criteria |
|----|----------|-----------|---------------|
| SY-1 | Fresh full-node V2 sync | New datadir, `--full`, sync from devnet/bepolia peers to tip, then follow for 24h | Reaches tip; stays at tip; `db settings` reports V2 |
| SY-2 | Fresh archive V2 sync | Default (archive) mode; then run the RPC diff campaign (SY-10) against a V1 archive reference | Historical queries equivalent |
| SY-3 | `--full` preset restarts | ≥ 20 stop/start cycles at random intervals while following tip | No crash loops, no prune-checkpoint regressions (v2.5.0 fixed both — verify on Berachain data) |
| SY-4 | `--minimal` preset | Same restart discipline; verify RPC errors for pruned ranges are clean and include block numbers (new v2.5.0 error shape) | Downstream tooling tolerates the new error text |
| SY-5 | Custom `prune.toml` | Distance-based and block-based history pruning, receipts log-filter for the deposit contract | Checkpoints progress monotonically over a multi-day run |
| SY-6 | V1 datadir long-run on new binary | Un-migrated node following tip for ≥ 1 week | No degradation vs V2 peers; confirms the "staying on V1" operator path |
| SY-7 | migrate-v2 at real scale, per node class | Migrate a full-node, an archive, and a pruned datadir (bepolia + mainnet copies). Record: wall time, peak extra disk (expect up to ~1x datadir headroom), final footprint delta | Post-migration: `db settings` = V2, node syncs, RPC diff vs pre-migration snapshot is clean. Publish Berachain-specific duration/size numbers for operator comms |
| SY-8 | Interrupted migration | `kill -9` mid-`migrate-v2`; restart the command; also try starting the node without completing migration | Behavior documented (resume/idempotent re-run/clear error); datadir never silently corrupted; runbook updated |
| SY-9 | Cross-version P2P | Old node syncing exclusively from new peers and vice versa; long-run peering soak | Header/body/receipt serving works both ways; no unexpected disconnect storms from v2.5.0's stricter devp2p validation (Hello identity binding, message-ID range checks); serving snap/2 requests never crashes even though Berachain doesn't use snap sync |
| SY-10 | RPC equivalence campaign | Paired old-V1 vs new-V2 nodes at the same height; scripted JSON diff over: `eth_getBlockByNumber` (hydrated), `eth_getBlockReceipts` (must include type-0x7e PoL receipts), `eth_getLogs` windows, `eth_getTransactionByHash`/receipt for old txs, `debug_traceBlockByNumber` (default + callTracer) **specifically on blocks containing the PoL tx** (v2.5.0 reuses one EVM across block replay — validates our block-scoped context), `eth_call` + `eth_getProof` at historical tags, `eth_feeHistory` | Byte-identical responses; every diff triaged and explained before rollout |
| SY-11 | Beacon-kit auxiliary flows | Deposit-log syncing (`eth_getLogs` on the deposit contract), node-api queries, genesis bootstrap against the new EL | All beacon-kit services healthy with a v2.5.0 EL underneath |

## 6. Disaster recovery and downgrade drills

| ID | Drill | What to establish |
|----|-------|-------------------|
| DR-1 | `kill -9` / power loss | Repeated hard kills on validators and full nodes during writes; static-file consistency healing on restart; node returns at the finalized tip with no divergence |
| DR-2 | Disk exhaustion | Fill the disk during normal V2 operation and during a migration; node/tool fails cleanly, recovers after space is freed |
| DR-3 | **`beacond rollback` vs `-38006`** (critical) | On a devnet validator: roll back CL state 1–2 heights, restart. v2.5.0 rejects FCU below the EL-finalized block with `-38006` — determine what beacon-kit sends as finalized, whether rollback works at all without EL-side action, and define the paired procedure (EL unwind command, resync, or tooling from beacon-kit #3156). This is a behavior change vs v1.11.4 that directly affects mainnet incident response. Finalize the runbook |
| DR-4 | Binary downgrade matrix | (a) New binary on V1 un-migrated datadir → downgrade to v1.4.x: verify the old binary still opens and syncs the datadir (2.5 may have bumped internal versions — test empirically, don't assume); (b) old binary pointed at a V2 datadir → verify clean refusal, no corruption; (c) migrated validator "downgrade" = restore pre-migration backup + resync the gap — time it at realistic sizes |
| DR-5 | V2 backup/restore | Define and rehearse consistent backups (MDBX + RocksDB + static files must be snapshotted together, node stopped); restore and catch up |

## 7. reth v2.5.0 gotcha checklist

| Upstream change | Risk for Berachain | Covered by |
|---|---|---|
| FCU below finalized rejected with `-38006`; payload construction allowed on ancestors ≥ finality (#26567) | `beacond rollback` runbook breaks; round-retry building must still work | DR-3, CV-9 |
| `memory-block-buffer-target` default 0→5 (#26462); `persistence-threshold` default 2→7 | Pinned to 0 in `init_engine_defaults()`; operators with bare flags must get pre-upgrade behavior | CV-10 (+ unit test) |
| Opt-in perf features: `--engine.sender-recovery-cache`, `--engine.txpool-prewarming` | Must stay **off** for behavior parity; optionally canary one non-validator to evaluate for a future release | CV-10 verification; optional canary |
| revm 42 EIP-8037 internal gas split | 30M system-call pin keeps `gasleft()` stable for PoL | CV-5 |
| Payload ID includes `target_gas_limit` | Stale-limit payload-cache bug closed; IDs differ from old EL for the same attributes (opaque to CL, but log tooling comparing IDs across versions will mismatch) | CV-6 |
| Blob-cell availability tracking in the pool (#25463) | Pool regression surface for 4844 txs | CV-8 |
| Stricter devp2p: Hello identity binding (#26639), message-ID range rejection (#26654) | Marginal/old peers could get dropped | SY-9 |
| snap/2 serving now real (#26339) | New serving code path exposed to the network | SY-9 |
| Pruning fixes: restart crash loops (#26565), checkpoint monotonicity (#26505, #26475/6) | Verify on Berachain data, not just trust the fix | SY-3/4/5 |
| Pruned-history errors now include block numbers (#26550) | RPC error-shape change may break error-parsing clients | SY-4, SY-10 |
| `eth_simulate` implicit gas caps (#26502); debug trace error codes aligned (#26479) | Dapp/indexer-visible RPC behavior changes | SY-10 triage |
| Single EVM reused across block replay tracing (#26614) | Explicitly fixes block-scoped-EVM chains like ours — but trace PoL blocks to prove it | SY-10 (PoL blocks) |
| New endpoints: `eth_getMultiProof` (#26555), `debug_getRawBlockAccessList` (#26438) | Berachain headers carry no BAL — endpoints must fail/return empty cleanly, not panic | SY-10 smoke |
| IPC decoding rework (#26540) | Only if any operator dials the engine/eth API over IPC — smoke large payloads | optional smoke |
| ERA import `--with-receipts` (#26436) | N/A — no Berachain ERA files; keep out of runbooks | docs only |

## 8. Load and soak profiles

- **Profiles**: idle baseline; sustained transfer/ERC-20 mix at ~50% gas target;
  gas-target saturation; blob-heavy; deposit bursts; PoL is exercised every block by
  construction. Rotate profiles during long soaks.
- **Durations**: smoke = 1h per scenario; regression = 24h; release soak = 72h+ on the
  custom-validator devnet and 1–2 weeks of bepolia canary.
- Fold operational events into every soak: rolling restarts, one live migration (CV-4),
  one rollback drill (DR-3).

## 9. Observability watchlist

- **Consensus**: missed heights / round counts per validator flavor; `getPayload` build
  time; `newPayload` and FCU latency p50/p99.
- **Performance**: block-processing latency before/after (upstream claims 5–10%
  improvement — confirm no regression on Berachain workloads, ideally capture the win).
- **Storage**: datadir size trend split by component (MDBX / RocksDB / static files);
  RocksDB compaction stalls; persistence duration; prune checkpoint progression.
- **Engine errors**: counters for `-38005` (expected pre-Osaka `getPayloadV5` only),
  `-38006` (should be zero outside rollback drills), any HTTP-level errors (must be zero
  — fatal to beacon-kit).
- **P2P**: peer churn and disconnect reasons (stricter validation), serving rates.
- **Process**: RSS (buffer target 0 should keep the in-memory block window flat vs
  upstream's new default of 5), fd counts, restart counts.

## 10. Exit criteria for mainnet rollout

1. Zero consensus divergence across all mixed-binary and mixed-storage soaks (≥ 72h
   under load), with every node flavor having proposed successfully.
2. Incremental-migration rehearsal (CV-4) completed at least twice, including one
   failure-injection run recovered from backup, with liveness maintained throughout;
   per-node downtime numbers published.
3. `beacond rollback` procedure against a v2.5.0 EL validated and documented (DR-3).
4. Downgrade matrix (DR-4) empirically verified; migrated-node restore drill timed.
5. RPC equivalence campaign clean: every diff explained; PoL receipts and traces
   byte-stable across versions and storage layouts.
6. Pruned-node restart soaks (≥ 20 cycles per preset) with no crash loops or checkpoint
   regressions.
7. Berachain-specific migration metrics (duration, peak disk, final footprint) published
   for operator communications, per datadir class.
8. Runbooks updated: migration, backup/restore, rollback, downgrade — reviewed by the
   infra team.

## Appendix: frequently used commands

```bash
# In-place migration (node stopped)
bera-reth db --datadir <datadir> migrate-v2 --chain <genesis.json|mainnet|bepolia>

# Inspect stored storage layout
bera-reth db --datadir <datadir> --chain <genesis> settings

# Local two-client smoke test
BEACON_KIT_PATH=../beacon-kit ./scripts/test-block-progression.sh

# Force pre-upgrade engine behavior explicitly (defaults are already pinned to 0)
bera-reth node --engine.persistence-threshold 0 --engine.memory-block-buffer-target 0 ...

# Opt a NEW datadir back into V1 (escape hatch only)
bera-reth node --storage.v2=false ...
```
