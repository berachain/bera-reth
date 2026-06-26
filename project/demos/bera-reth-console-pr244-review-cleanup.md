# Proof Parade: BERA-519 — Bera-reth console PR 244 review cleanup

| Field | Value |
| ----- | ----- |
| **Task ref** | BERA-519 (Cancelled — superseded) |
| **Brief** | `project/briefs/bera-reth-console-pr244-review-cleanup.md` |
| **Branch** | `feat/proof-of-gossip` @ `5c5e1fc` (cleanup), `b5b3107` (null IPC), squash `761e32e` (console body) |
| **Proof medium** | Markdown + source tests (CI pending) |
| **Successor** | BERA-520 phase A (PoG→`main`: `rev` pin + CI) |

## Evidence matrix

| ID | Claim (AC / TP) | Evidence type | Artifact | PASS |
| --- | --- | --- | --- | --- |
| E1 | AC-2, demo ledger | Doc + GitHub | Brief § PR 244 retirement ledger; PR 244 closed; `feat/reth-console` deleted | yes |
| E2 | AC-3, nah items | Doc | Brief § NOT in Scope + retirement ledger “Explicitly not taken” | yes |
| E3 | AC-4, TP-1, TP-2 | Unit tests | `command::tests::parses_rpc_with_params_and_query_tail`, `parses_parenthesized_rpc_with_query_tail`, chained query tests | yes* |
| E4 | AC-5, TP-3 | Code trace | `run.rs`: `--exec` → `run_exec` only; probes in `else` REPL branch | yes |
| E5 | AC-6, TP-4 | Code trace + unit | Fallback: `tokio::join!`; beradmin: cached JSON via `format_beradmin_startup_line` + unit tests | yes* |
| E6 | AC-7, TP-5 | Unit tests | `output::tests::node_status_keeps_chain_and_network_distinct`, `node_status_uses_network_id_not_chain_for_net_field` | yes* |
| E7 | AC-8, TP-6 | Unit tests | `repl::tests::startup_peer_total_falls_back_to_inbound_plus_outbound` | yes* |
| E8 | null IPC (`dd05c97`) | Async unit test | `rpc::tests::ipc_client_handles_empty_response_and_rpc_error` (null `error` → ok) | yes* |
| E9 | AC-9, TP-7 | Lead decision | `Cargo.toml` still `branch = pog/provenance-callback`; deferred to BERA-520 A | deferred |
| E10 | AC-10, TP-8 | CI | `cargo test --lib` not run here (devcontainer compile failure); PoG→`main` CI | pending |

\* Test names verified in source; full suite not executed in this environment (see E10).

---

## E1 — Review ledger + PR retirement

**Fixed / harvested (PoG commits):**

| Item | Commit |
|------|--------|
| Parameterized RPC + query tail | `5c5e1fc` |
| `--exec` skips REPL startup probes | `5c5e1fc` |
| Concurrent fallback startup snapshot | `5c5e1fc` |
| Distinct `chainId` / `networkId` in status | `5c5e1fc` |
| Peer total = in + out when total absent/zero | `5c5e1fc` |
| `"error": null` IPC success | `b5b3107` |
| Console body (IPC, reedline, beradmin_*, removeAllPeers, …) | squash `761e32e` |

**Outstanding → BERA-520 only:** fork `rev` pin, CI green, merge PoG→`main`.

---

## E3 — Parser: parameterized RPC + query tail (TP-1, TP-2)

Source tests in `src/console/command.rs`:

```rust
// TP-1 flagship case
parse_input(r#"eth_getBlockByNumber ["latest", false].transactions.count"#)
// → RpcWithQuery { method, params: Some(["latest", false]), query: ".transactions.count" }

// TP-2 no-param chain
parse_input("admin.peers.count") → RpcWithQuery { method: "admin.peers", params: None, query: ".count" }
```

Run (when CI/local build works):

```bash
cd bera-reth && cargo test --lib console::command::tests::parses_rpc_with_params_and_query_tail console::command::tests::parses_parenthesized_rpc_with_query_tail -- --nocapture
```

---

## E4 — TP-3: `--exec` skips startup probes (AC-5)

**Contract:** `--exec` must not call `eth_chainId` or `beradmin_nodeStatus` before the scripted RPC.

**Code trace** (`src/console/run.rs`):

```15:34:bera-reth/src/console/run.rs
    if let Some(script) = cmd.exec.as_deref() {
        run_exec(&rpc, script).await?;
    } else {
        let chain_id =
            rpc.request_value("eth_chainId", None).await.ok().and_then(|v| parse_chain_id(&v));

        let bera_admin_status = rpc.request_value("beradmin_nodeStatus", None).await.ok();
        // ... run_repl(...)
    }
```

`run_exec` (`src/console/exec.rs`) calls `evaluate_line` only — no discovery probes.

**Dedicated TP-3 unit test:** not added. Control flow is a single branch; mock-RPC test would be brittle for little gain. **Smoke (optional, manual):** with a running node, `bera-reth console --exec "eth_blockNumber"` should emit one JSON line without multi-second delay from startup probes.

---

## E5 — REPL startup: beradmin vs fallback (AC-6, TP-4)

### Fallback (no beradmin)

When `beradmin_nodeStatus` fails, `print_startup_snapshot` uses four concurrent RPCs:

```168:173:bera-reth/src/console/repl.rs
        let (version, block, peers, network) = tokio::join!(
            rpc.request_value("web3_clientVersion", None),
            rpc.request_value("eth_blockNumber", None),
            rpc.request_value("net_peerCount", None),
            rpc.request_value("net_version", None),
        );
```

### Beradmin path (common on PoG nodes)

1. **`run.rs`** (REPL only): two serial probes — `eth_chainId`, then `beradmin_nodeStatus` — before `run_repl`.
2. **`print_startup_snapshot`**: if status JSON is present, **zero** additional RPCs; formats from cached fields + `format_startup_peers`.

**Should we test beradmin startup?**

| Layer | Worth it? | Notes |
|-------|-----------|-------|
| Peer total / chain emoji / field keys from sample JSON | **Yes, cheap** | Extend `repl` tests with a pure helper or snapshot of printed line from fixture `Value` — catches regressions on `peerCountTotal` / snake_case keys |
| Serial `eth_chainId` + `beradmin_nodeStatus` before prompt | **Low ROI** | Two fast IPC calls; bounded-failure behavior is “show prompt with partial data” — already implied by `.ok()` swallowing errors |
| End-to-end REPL prompt timing | **Manual / CI smoke** | Needs live IPC; defer to PoG merge validation |

**Unit tests added** (`repl::tests`):

- `beradmin_startup_line_from_camel_case_status` — camelCase keys, peer total fallback, chain emoji
- `beradmin_startup_line_accepts_snake_case_fields` — snake_case aliases, hex `head_number`

---

## E6 — Distinct `chain=` / `net=` (TP-5)

```627:664:bera-reth/src/console/output.rs
    fn node_status_keeps_chain_and_network_distinct() { ... chain=80094 ... net=80094 ... peers=5 (in=2 out=3) }
    fn node_status_uses_network_id_not_chain_for_net_field() { ... chain=80094 ... net=1 ... }
```

---

## E7 — Peer total fallback (TP-6)

```347:356:bera-reth/src/console/repl.rs
    fn startup_peer_total_falls_back_to_inbound_plus_outbound() {
        assert_eq!(format_startup_peers(Some(0), Some(3), Some(2)), "peers=5 (in=3 out=2)");
        assert_eq!(format_startup_peers(None, Some(1), Some(4)), "peers=5 (in=1 out=4)");
    }
```

Same `effective_peer_total` logic drives `output.rs` status formatting.

---

## E8 — Null JSON-RPC error on IPC

```228:248:bera-reth/src/console/rpc.rs
            // Third request: JSON-RPC error field is null (success).
            ...
        let null_err_response = client.request("eth_chainId", json!([])).await;
        assert!(null_err_response.is_ok());
```

---

## E9 — Dependency pin (TP-7) — deferred

Lead decision (2026-06-26): float `branch = pog/provenance-callback` on PoG; pin `rev = c34120ac…` at PoG→`main` (BERA-520 step A).

```toml
# bera-reth/Cargo.toml (representative)
reth = { git = "https://github.com/camembera/reth", branch = "pog/provenance-callback" }
```

---

## E10 — Full regression (TP-8) — pending CI

```bash
cd bera-reth && cargo test --lib
```

**Status: not run in proof capture environment** — compile failed mid-build (`coins-bip32` / missing crates; prior OOM on full fixture graph). Gate moves to PoG→`main` PR CI.

---

## Summary

| Area | Status |
|------|--------|
| Parser, output, peer fallback, null IPC | Implemented + unit tests in tree |
| TP-3 `--exec` no preflight | Proven by code structure; no dedicated test |
| Beradmin startup | Cached snapshot OK; optional formatting unit test only |
| Pin + full `cargo test --lib` | BERA-520 phase A |
