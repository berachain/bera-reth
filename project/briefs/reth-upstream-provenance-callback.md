# Brief: Upstream merge — `TransactionProvenanceSink` in reth

## Problem

`bera-reth` Proof-of-Gossip attribution depends on a **pluggable hook in reth’s transaction gossip path**. Today that hook lives on a **`camembera/reth` fork** (`pog/provenance-callback`, 6 commits on **`v1.11.4`**, tip `c34120ac`).

The fork already implements the right shape: optional `TransactionProvenanceSink`, default `None`, invoked **after the pool accepts** external transactions. **Berachain policy and storage stay in `bera-reth`** (`PogTxProvenanceSink`, sealed-tx-fact SQLite, sentinel export).

**Strategic choice:** pursue **upstream merge into `paradigmxyz/reth`**, not long-term fork maintenance.

**Sequencing (Lead, 2026-06-26):** **Do not open an upstream PR until `bera-reth` has migrated to reth 2.x.** Until then, maintain the fork on PoG. Port the minimal patch onto **`paradigmxyz/reth` `main` (2.x)** as prep work only; open the PR after the 2.x migration lands (or is imminently merging).

**Constraint:** **minimize diff size** — smallest patch that upstream can review and merge; defer or fork-only anything not required for the core hook.

## Current patch inventory (fork tip vs `v1.11.4`)

| Commit | Lines (stat) | Upstream? | Notes |
|--------|----------------|-----------|-------|
| `e1e76680e` | +100 | **Yes (core)** | Trait, `with_provenance_callback`, builder entry |
| `c513a9b08` | small | **Yes (core)** | Fire callback after pool accept, not before |
| `8eadd34e6` | small | **Yes (core)** | Typing / signature alignment |
| `70a7c3801` | +50/−41 churn | **Rewrite, don’t port** | Dedupe refactor — resubmit as minimal additive diff |
| `59ab110bc` | +40 | **No (fork-only)** | `post_known_peers_write` — PoG peer curation on shutdown; separate concern |
| `c34120ac2` | +302 | **Phase 2 or slim** | `listening_addr` session plumbing + tests — **largest chunk** |

**Total today:** 12 files, +462 / −8. **Target upstream PR 1:** ~**110–130 net lines** (core callback, `peer_id` + accepted hashes). **Target PR 2 (if split):** session → callback `listening_addr` propagation (~250 net, tests trimmed).

## Approach

### Upstream pitch (one sentence)

> Optional `TransactionProvenanceSink` invoked after successful external tx pool import, keyed by `(peer_id, accepted_hashes)`; default disabled; no chain-specific behavior in reth.

### Diff minimization rules

1. **One concern per upstream PR** — provenance callback only; **exclude** `post_known_peers_write` (keep on `camembera/reth` until bera-reth replaces shutdown curation or upstream accepts a separate hook PR later).
2. **Resubmit, don’t cherry-pick** — squash fork commits into clean additive diffs; **drop** `70a7c3801` refactor churn; port onto **`paradigmxyz/reth` `main` (2.x)** when preparing the upstream PR (not `v1.11.4` backport).
3. **Neutral surface** — trait/module docs describe “transaction provenance / attribution hook”; remove Berachain / PoG / BERA-* references from reth-side comments.
4. **Minimal tests upstream** — one unit/integration test: callback receives `(peer_id, hashes)` after pool accept; `None` callback unchanged behavior. Move BERA-305 session tests to `bera-reth` or fork until PR 2.
5. **Phase `listening_addr`** — if reviewers push back on size, **PR 1** ships `(peer_id, &[TxHash])` only; **PR 2** adds `Option<SocketAddr>` from devp2p Hello (required for `first_enode` in PoG). Document interim: `first_enode` NULL until PR 2 merges (peer_id attribution still works).
6. **bera-reth stays thin** — `impl TransactionProvenanceSink for PogTxProvenanceSink` only; no reth policy in the fork.

### Target branch — decided

**`paradigmxyz/reth` `main` (2.x) only.** No PR against `v1.11.4`.

| Phase | When | Action |
|-------|------|--------|
| **Now → PoG→`main`** | `bera-reth` still on **1.11.x** | Keep `camembera/reth` fork (`pog/provenance-callback`); pin `rev` at merge |
| **bera-reth 2.x migration** | Blocker for upstream | Move `bera-reth` `main` (+ PoG) to `paradigmxyz/reth` 2.x; rebase fork patch onto 2.x or drop fork if ported inline |
| **Upstream PR** | **After** 2.x migration | Open minimal PR 1 (+ PR 2 if split); socialize API in issue/Discord immediately before or with PR |

Until 2.x migration: fork maintenance is **expected**, not a failure mode.

### Fallback if upstream rejects or stalls

- Keep **`camembera/reth` `pog/provenance-callback`** pinned on PoG (`rev` at PoG→`main`).
- Revisit upstream when `bera-reth` migrates to reth 2.x or maintainers signal appetite.
- **`post_known_peers_write` remains fork-only** indefinitely unless separately proposed.

## Context Payload

- **Fork repo:** `https://github.com/camembera/reth`, branch `pog/provenance-callback`, tip `c34120ac`
- **Base:** `v1.11.4` (`2ac58a25f`) + 6 commits above
- **bera-reth consumers:**
  - `src/node/mod.rs` — `PogTxProvenanceSink`, `start_network_with_provenance_callback`
  - `src/pog/mod.rs` — inflight + sealed-tx-fact; `post_known_peers_write` hook (fork-only today)
- **bera-reth `main` reth dep:** `paradigmxyz/reth` tag `v1.11.4` (no patch)
- **bera-reth PoG reth dep:** `camembera/reth` branch (→ pin `rev` at PoG→`main`)
- **Related briefs:** `bera-sentinel` BERA-305 peer attribution / enode pipeline; `bera-reth` console brief (retired PR 244)

## Public Contract (upstream API)

### PR 1 — minimal merge target

```rust
pub trait TransactionProvenanceSink: Send + Sync {
    fn record_accepted_from_peer(
        &self,
        peer_id: PeerId,
        accepted_tx_hashes: &[TxHash],
    );
}
```

- `TransactionsManager`: `Option<Arc<dyn TransactionProvenanceSink>>`, default `None`.
- Invoked **after** `pool.add_external_transactions` with **only successfully accepted** hashes.
- `NetworkBuilder::with_tx_provenance_callback` + `BuilderContext::start_network_with_provenance_callback` (or upstream-equivalent naming).
- **No behavior change** when callback unset.

### PR 2 — optional follow-up (PoG enode column)

Widen signature to include `listening_addr: Option<SocketAddr>` (Hello.port + remote IP at session establish). Required for canonical `first_enode` in sealed-tx-fact; **not** required for peer_id-level attribution.

### Explicitly not in upstream provenance PR(s)

- `set_post_known_peers_write_hook` / known-peers curation
- PoG SQLite, sentinel, `beradmin_*` RPC
- Berachain-specific metrics or policy

## Allowed Implementation Latitude

- Trait/module naming may follow reth conventions (`TxProvenanceObserver`, etc.) if reviewers prefer.
- Builder wiring may use existing extension patterns instead of new `start_network_with_*` if equivalent.
- PR 1 may land without `listening_addr`; bera-reth adapts temporarily.

## NOT in Scope

- **Full reth 2.x migration for `bera-reth`** — separate program; may follow upstream merge.
- **Upstream `post_known_peers_write`** — fork-only unless explicitly briefed later.
- **Console work** — done on PoG; unrelated to this brief.
- **Replacing fork before upstream merge** — fork stays until PR merges and `bera-reth` switches deps.

## Test Plan

### Upstream PR 1

- [ ] **TP-1** Default node: no callback registered; existing tx gossip tests pass.
- [ ] **TP-2** Recording sink: import batch from mock peer → callback receives exact accepted hash list.
- [ ] **TP-3** Failed pool imports not reported to callback.
- [ ] **TP-4** `cargo test -p reth-network` (and affected crates) green.

### Upstream PR 2 (if split)

- [ ] **TP-5** `Hello.port != 0` → `Some(SocketAddr)` on callback.
- [ ] **TP-6** `Hello.port == 0` → `None`.

### bera-reth after upstream tag available

- [ ] **TP-7** PoG enabled: `PogTxProvenanceSink` receives events; sealed-tx-fact rows populated.
- [ ] **TP-8** Switch PoG from `camembera/reth` git dep to `paradigmxyz/reth` tag containing merge; drop fork for provenance (keep fork-only hook if still needed).

## Acceptance Criteria

### Before upstream PR (now → 2.x migration)

- [ ] **AC-0** PoG→`main` merges with fork pinned (`rev`); provenance works on `camembera/reth`.
- [ ] **AC-0b** `bera-reth` 2.x migration tracked and scheduled (prerequisite for upstream PR).

### Upstream PR (after 2.x migration)

- [ ] **AC-1** Patch ported to 2.x with minimal diff (~110–130 lines PR 1 target); optional maintainer pre-discussion.
- [ ] **AC-2** PR 1 opened with **≤ ~150 net lines** in reth (excluding tests); no Berachain references in reth crate docs.
- [ ] **AC-3** `post_known_peers_write` **not** included in provenance PR.
- [ ] **AC-4** PR 1 merged **or** explicit rejection with documented fallback (maintain fork on 2.x base).
- [ ] **AC-5** If PR 1 merges without `listening_addr`: PR 2 opened or enode gap accepted with metric/ops plan.
- [ ] **AC-6** `bera-reth` builds against released reth containing merge; fork dep dropped for provenance (fork-only hooks if any remain).

## Deployment Plan

### Phase A — now (no upstream PR)

1. PoG→`main` with `camembera/reth` fork + `rev` pin.
2. Keep fork rebased on **`v1.11.4` + patch** until `bera-reth` 2.x migration starts.
3. Optionally draft minimal 2.x port on a branch (no public PR).

### Phase B — `bera-reth` reth 2.x migration

1. Move `bera-reth` `main` to `paradigmxyz/reth` 2.x.
2. Rebase `pog/provenance-callback` onto 2.x **or** carry patch as a local branch off `main`.
3. PoG follows `main`; verify `PogTxProvenanceSink` + sealed-tx-fact on 2.x.

### Phase C — upstream PR (after Phase B)

1. Socialize API in reth issue/Discord.
2. Open PR 1 (minimal diff); PR 2 for `listening_addr` if split.
3. On merge: switch `bera-reth` to stock reth tag; retain fork only for `post_known_peers_write` if still needed.

## References

- Fork diff stat: `git diff v1.11.4..c34120ac2 --stat` (12 files, +462)
- Core-only diff: `git diff v1.11.4..8eadd34e6 --stat` (~4 files, +111)
- `reth/crates/net/network/src/transactions/provenance.rs`
- `bera-reth/src/node/mod.rs` — `PogTxProvenanceSink`
- Console / PR 244 retirement: `project/briefs/bera-reth-console-pr244-review-cleanup.md`

## Tracking

- **Task ref:** TBD (upstream provenance callback)
- **Scope mode:** **DEFERRED** — upstream PR blocked on `bera-reth` reth **2.x migration**
- **Strategic decision:** Try upstream merge on **2.x** after migration; minimize diff; fork is interim delivery until then
