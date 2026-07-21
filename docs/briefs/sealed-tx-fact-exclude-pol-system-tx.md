# Brief: sealed_tx_fact must exclude PoL / system transactions

## Problem

`bera-reth`'s seal-flush path inserts one `sealed_tx_fact` row per transaction in
every locally-built block, indiscriminately of `tx_type`. On bepolia today
(28 Apr 2026 sample, `pg-bepolia-pruned` `proof_of_gossip.db`):

```
total      = 141
attributed = 2     (real EIP-1559 user txs to Multicall3)
non-attrib = 139   (≥137 confirmed via RPC: type=0x7e, from=0xff…fffe, to=PoL distributor)
```

PoL distribution transactions in Berachain are
[`BerachainTxType::Berachain` / type `0x7e`](../../src/transaction/mod.rs)
(`src/transaction/mod.rs:282-292`), system-injected by the EL/CL
rewards path. They never traverse the eth/68 mempool gossip, so their inflight
lookup misses and their row is written with `(first_peer_id, first_enode) = (NULL, NULL)`.
The resulting noise:

- **>97% of `sealed_tx_fact` rows are unattributed system txs.** Operator
  queries (e.g. "what fraction of my sealed txs were peer-attributed?") read
  meaningless ratios; the BERA-305 `pog_sealed_tx_facts_flushed_first_enode_total`
  buckets are dominated by `outcome=null_no_peer` from PoL.
- **`effective_tip_wei` is always `0x0`** for these rows (PoL txs pay no
  base-fee-relative tip). They drag the tip-source aggregation toward zero with
  no semantic content.
- **Retention churn.** The 7-day retention sweep deletes ~one PoL row per
  block × 86,400 blocks/day across the fleet — pure SQLite write amplification
  with zero analytic value.
- **Sentinel mirror amplification.** `beradmin_exportSealedTxFacts` ships these
  rows over the operator console RPC; sentinel mirrors them into
  `sealed_tx_facts` and the WS `attribution_summary` reports inflated
  `facts_non_p2p` (137 of 139 in the cited sample). The Attribution tab is
  technically correct but the totals are misleading.
- **Conflates two failure modes under `null_no_peer`.** Today the bucket
  collapses (a) "locally-built / RPC-only" txs that legitimately have no peer
  and (b) "system tx that should never have been a row at all." Operator
  diagnostics for a real first-hear regression get drowned in PoL volume.

The right cut is to skip system transactions at the seal-flush filter step so
they never become rows. `bera-reth` already has the type discriminant in hand:
`BerachainTxEnvelope::Berachain(_)` is exactly the set we want to drop.

## Approach

After this work:

- `run_seal_flush_from_canon` (`src/pog/mod.rs:1353`) iterates
  `body.transactions_iter()` and **skips any `BerachainTxEnvelope::Berachain(_)`
  variant** before computing `tx_hashes`, `effective_gas_prices`, and the
  receipt-aligned `tips` slice. Filtered txs do not appear in `sealed_tx_fact`
  at all.
- The retention DELETE on empty blocks is preserved (no rows in, sweep still
  runs; brief §current behavior).
- Receipt alignment is maintained by filtering both `transactions_iter()`
  and the parallel `receipts_by_block` slice with the same predicate, in the
  same iteration order. Tests cover the alignment.
- A single `pog_sealed_flush_tx_skipped_total{reason="system_tx"}` counter is
  incremented per filtered tx so operators can see PoL volume out of band.
- The existing four buckets on
  `pog_sealed_tx_facts_flushed_first_enode_total` (`present`, `null_missing_address`,
  `null_no_peer`) keep their meanings; with PoL filtered out, `null_no_peer`
  now exclusively reflects locally-injected / RPC-only txs (canaries,
  `eth_sendRawTransaction` to the local node before gossip closes the loop).

Out of scope (decision points, deferred or rejected — see *NOT in Scope*):
filtering by `from = SYSTEM_ADDRESS` (more brittle than tx-type discriminant);
filtering legacy/access-list/blob (0x0/0x1/0x3) — those are real user txs and
must remain; backfill-deletion of historical PoL rows in deployed `proof_of_gossip.db`
files; sentinel-side filter on the mirror.

## Context Payload

- **Target Files (bera-reth):**
  - `src/pog/mod.rs` — `run_seal_flush_from_canon`
    (~1353-1408), `build_sealed_tx_fact_inserts` (~1432-1463). Add filter +
    skip-counter at the iteration site; thread the filtered `(tx, receipt)`
    pairs through to tip computation.
  - `src/transaction/mod.rs:282-308` — `BerachainTxEnvelope`
    enum and `tx_type()` accessor. Read-only; this is the discriminant.
  - `src/transaction/txtype.rs` — `POL_TX_TYPE = 126`.
    Read-only.
- **Reference (read-only, do not modify):**
  - `bera-sentinel/src/sealed_tx_facts.rs` (mirror) — confirms sentinel
    has no tx-type column; must filter at source.
  - `bera-sentinel/docs/briefs/peer-attribution-enode-pipeline.md` —
    BERA-305 brief; this defect was surfaced during its implementation review.
- **Required Context (read first):**
  - `.cursor/rules/issue-tracking.mdc` and `.cursor/rules/helm.mdc` — Briefed
    Lifecycle, Quality Floor.
  - BRIP-0004 — defines the Berachain PoL tx type (0x7e); referenced by
    `BerachainTxEnvelope::Berachain` doc comment.
  - BERA-305 — the consumer of `sealed_tx_fact` whose attribution semantics
    motivate the filter.
- **Test Command:**
  - `cargo test -p bera-reth pog::` (pog module tests)
  - `cargo test` (full suite, gates Pre-Commit)

## Public Contract

This task changes the *content* (not shape) of two existing surfaces:

### `sealed_tx_fact` SQLite table

- **Stored Shape:** Unchanged. No schema migration.
- **Content Change:** Rows where the underlying tx is type `0x7e`
  (`BerachainTxEnvelope::Berachain`) are no longer written. Existing historical
  rows are not retroactively deleted; retention sweep eventually ages them out
  (7-day default).
- **Compatibility:** Consumers (`beradmin_exportSealedTxFacts`,
  bera-sentinel mirror, BERA-305 first_enode buckets) are forward-compatible
  — fewer rows, same shape. A consumer that *counted on* PoL rows being
  present (none known) would break.

### `pog_sealed_flush_tx_skipped_total` (new counter)

- **Stored Shape:** Prometheus counter labelled
  `reason ∈ {"system_tx"}`. Increments by 1 per filtered tx at seal-flush
  time. `system_tx` is the only label initially; the dimension exists so
  future filter reasons can be added without breaking dashboards.
- **Compatibility:** Additive.

### `pog_sealed_tx_facts_flushed_first_enode_total` buckets

- **Stored Shape:** Unchanged. No new bucket.
- **Semantic Change:** `outcome=null_no_peer` no longer includes PoL system
  txs. After deploy this counter's rate drops sharply on validators that build
  blocks with PoL distributions; operators reading absolute rates need to
  re-baseline. Documented in the deploy plan.

## Domain Standards

- **Engineering**: Consulted — AC-1..AC-5 enforce in-repo unit coverage of
  the filter predicate, receipt alignment, and the new metric. AC-6 is the
  adversarial-review gate. No live integration test required by AC; live
  behavior is validated under VC-1.
- **Deployment**: Consulted — no schema migration, no operator action. Rollback
  is a binary revert; historical PoL rows already absent from `sealed_tx_fact`
  remain absent (no replay).
- **Security**: N/A — no auth or secret changes; filter is on a publicly-
  observable tx-type discriminant.
- **Other**: N/A.

## Demonstration Plan

After deploy on the playground (one validator, e.g. `pg-bepolia-pruned`):

- [ ] Wait ≥10 blocks containing PoL distributions. Assert
      `select count(*) from sealed_tx_fact where first_peer_id is null and ingested_at > <deploy_ts>;`
      drops to "≈ canaries injected since deploy" rather than tracking block count.
- [ ] Capture metrics: `pog_sealed_flush_tx_skipped_total{reason="system_tx"}`
      increases monotonically; `pog_sealed_tx_facts_flushed_first_enode_total{outcome="null_no_peer"}`
      growth rate falls by the same magnitude.
- [ ] Sentinel WS `attribution_summary.facts_non_p2p` drops to a number
      proportional to canaries / RPC-injected txs only.

**Proof Parade (evidence matrix):** `project/demos/BERA-325-sealed-tx-fact-exclude-pol-system-tx.md` in the portfolio `project` repo (multi-root `~/src` layout).

## Test Plan

- [ ] **TP-1** Unit (new): `seal_flush_skips_pol_system_tx` in `pog/mod.rs`
      tests. Synthetic block with one EIP-1559 tx and one PoL tx; receipts
      aligned. Call `build_sealed_tx_fact_inserts` (or extracted filter helper)
      and assert exactly one row is produced, corresponding to the EIP-1559
      tx. Must fail on the pre-change codebase (returns 2 rows) and pass on
      the post-change codebase.
- [ ] **TP-2** Unit (new): `seal_flush_filter_preserves_receipt_alignment`.
      Block of 4 txs in order `[Eth1559(a), PoL(b), Eth1559(c), PoL(d)]` with
      synthetic receipts containing distinct `cumulative_gas_used` values.
      Filtered tip slice must contain tips for `(a, c)` only, computed from
      the receipts at indices `(0, 2)` (because `cumulative_gas_used` is a
      running total — the filter must subtract the *previous* receipt's
      cumulative, even when an intervening receipt was filtered out). This is
      the alignment hazard.
- [ ] **TP-3** Unit (new): `seal_flush_skip_counter_increments_per_filtered_tx`.
      Same input as TP-2; assert
      `pog_sealed_flush_tx_skipped_total{reason="system_tx"}` increments by
      exactly 2.
- [ ] **TP-4** Unit (extend existing seal-flush tests): all currently-passing
      seal-flush tests continue to pass — no behavior change for blocks that
      contain only Ethereum txs.
- [ ] **TP-5** Unit (new): `seal_flush_all_pol_block_writes_zero_rows`.
      Synthetic block of 3 PoL txs only; assert no `sealed_tx_fact` rows are
      inserted but the retention DELETE still runs (regression guard against
      the empty-block early-return at `mod.rs:1374-1379`).
- [ ] **TP-6** Reviewer gates: `reviewer_pass` / `reviewer_block` / `reviewer_carry`
      (MCP, per `issue-tracking.mdc`) — Plan Reviewer and Implementation Reviewer
      each record adversarial clearance for the matching gate (`plan_review`,
      `implementation_review`) with substantive summary; no `reviewer_findings` RPC.

## Acceptance Criteria

- [ ] **AC-1** PoL system txs (`BerachainTxEnvelope::Berachain`) are never
      written to `sealed_tx_fact`. Proven by TP-1, TP-5.
- [ ] **AC-2** Filter preserves receipt-tip alignment for the surviving
      Ethereum txs (cumulative-gas-used arithmetic accounts for filtered
      indices). Proven by TP-2.
- [ ] **AC-3** New counter `pog_sealed_flush_tx_skipped_total{reason="system_tx"}`
      increments by exactly one per filtered tx. Proven by TP-3.
- [ ] **AC-4** All existing `pog::` tests pass unchanged (no regression on
      Ethereum-only blocks). Proven by TP-4 + the existing test suite.
- [ ] **AC-5** Empty-after-filter blocks still trigger the retention DELETE.
      Proven by TP-5.
- [ ] **AC-6** Adversarial review passing per `issue-tracking.mdc` gates. TP-6.

## Validation Criteria

- [ ] **VC-1** Live PoL filter on playground bepolia. Run the filtered
      bera-reth on `pg-bepolia-pruned` for ≥30 minutes spanning ≥10 sealed
      blocks containing PoL distributions. Acceptance:
      `pog_sealed_flush_tx_skipped_total{reason="system_tx"}` increments at the
      block-PoL-tx rate; new `sealed_tx_fact` rows in the same window contain
      zero rows where `eth_getTransactionByHash(tx_hash).type == "0x7e"`.
      → evidence: SQLite query result + `curl /metrics` excerpt attached to the task.
- [ ] **VC-2** Sentinel mirror reflects the drop. The bera-sentinel
      `attribution_summary.facts_non_p2p` reported on the WS `state.update`
      drops by the same magnitude as the pre-deploy PoL rate within one
      `sealed_fact_export.limit` page after deploy. → evidence: WS payload
      excerpt before and after.

## Tracking

- **Task ref**: BERA-325
- **Scope mode**: HOLD — bug-fix-shaped, surfaced during BERA-305
  implementation review.

## NOT in Scope

- **Filter by `from = 0xff…fffe` (SYSTEM_ADDRESS).** Rejected; the tx-type
  discriminant is the canonical predicate, less brittle to address-shape
  changes, and matches the existing `BerachainTxEnvelope` enum exhaustively.
  *(No follow-up.)*
- **Filter to EIP-1559 only (drop legacy/access-list/blob).** Rejected;
  legacy (0x0), access-list (0x1), and blob (0x3) are legitimate user tx types
  that should appear in `sealed_tx_fact` with whatever attribution they
  earned. The user-stated heuristic ("we only want eip1559") was a paraphrase
  of "we don't want system tx" and the precise discriminant lives on the
  envelope, not the type byte. *(No follow-up.)*
- **Backfill-delete historical PoL rows on existing deployed nodes.**
  Deferred; 7-day retention ages them out naturally. If accelerated cleanup
  is needed, file a separate ops task. *(Follow-up: optional, file if asked.)*
- **Sentinel-side mirror filter.** Rejected; once the source is clean, the
  mirror is clean. Adding a parallel filter would be defense-in-depth at the
  cost of two places to keep in sync. *(No follow-up.)*
- **New `outcome=system_tx_skipped` bucket on `pog_sealed_tx_facts_flushed_first_enode_total`.**
  Rejected; the counter is for *flushed* rows, and we no longer flush these.
  The skip-count goes on the new dedicated counter. *(No follow-up.)*

## What Already Exists

- **`BerachainTxEnvelope::Berachain(Sealed<PoLTx>)`** discriminant
  (`src/transaction/mod.rs:289-291`). Reused as the filter
  predicate; not modified.
- **`POL_TX_TYPE = 126`** literal (`src/transaction/txtype.rs:3`).
  Reused via the envelope variant; the brief deliberately matches on the
  variant rather than the byte to stay structural.
- **Receipt-by-block provider** (`provider.receipts_by_block`,
  `mod.rs:1383-1385`). Reused; the alignment work happens in the same loop
  that already zips `effective_gas_prices` with `receipts`.
- **Existing `pog_sealed_*` metric families** in `pog/mod.rs`. The new
  `pog_sealed_flush_tx_skipped_total` is registered alongside, no
  re-registration of existing families.

## Error & Failure Map

- **Receipt-tx slice length mismatch after filter.** Defended by TP-2.
  Both slices are filtered with the same predicate in the same iteration;
  any divergence is a programming error caught at unit-test time.
- **`cumulative_gas_used` arithmetic.** Filtered receipts must NOT be skipped
  in the running-total computation — the engine's cumulative is over
  *all* txs in the block, including PoL. The filter applies to the
  *output* tips slice only; the running total still walks every receipt
  and discards the entries for filtered txs after computing their
  individual gas-used. TP-2 codifies this.
- **Block contains only PoL txs (genesis-style edge case).** TP-5 covers
  it. The seal-flush early-return on empty `tx_hashes` already runs the
  retention DELETE; the filtered case behaves identically post-filter.
- **Future PoL tx-type variants.** If `BerachainTxEnvelope` gains a new
  non-Ethereum variant, an exhaustive `match` on the enum will not compile
  until the brief's filter is updated. Use a `match` (not an `if let`) at
  the filter site to make this a compile-time gate. AC-1 implicitly covers it.

## Deployment Plan

1. **bera-reth only.** No coordinated bera-sentinel or beacon-kit change;
   no schema migration; no operator config change.
2. **Rollout:** drop-in binary replacement on each bera-reth deploy
   (playground first, then validators per the standard fleet rollout). New
   blocks immediately produce filtered `sealed_tx_fact`; existing rows age out
   under retention.
3. **Rollback:** revert binary. The new `pog_sealed_flush_tx_skipped_total`
   counter stops emitting; previously-skipped PoL rows resume being written.
   No data loss.
4. **Post-deploy verification:** VC-1 (filter live) and VC-2 (sentinel mirror
   reflects drop) on the playground deploy before fleet rollout.

## References

- [`src/pog/mod.rs:1353-1463`](../../src/pog/mod.rs)
- [`src/transaction/mod.rs:282-308`](../../src/transaction/mod.rs)
- [`src/transaction/txtype.rs`](../../src/transaction/txtype.rs)
- BERA-305 brief (sibling repo): `bera-sentinel/docs/briefs/peer-attribution-enode-pipeline.md`
- BERA-323 brief (sibling repo): `bera-sentinel/docs/briefs/probe-pacing-single-inflight.md`
  — companion deferred task surfaced during the same review.
- Surfacing conversation: [Reth provenance enode pipeline scoping](7a6f9bd9-e4b9-475d-97ad-9f96a0bc7a06).
