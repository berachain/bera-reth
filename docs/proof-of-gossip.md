# Proof of Gossip

`--bera.pog` installs the `pog` JSON-RPC namespace on IPC only. HTTP and
WebSocket never serve it. Without the flag, no `pog_*` method exists and no PoG
metric is published.

```bash
bera-reth node --bera.pog
```

Call the node's IPC socket (`/tmp/reth.ipc` by default).

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"pog_peers","params":[]}' \
  | nc -U /tmp/reth.ipc
```

The namespace records that this node sent one signed transaction to one connected
peer, then whether that hash later appeared in a block. It does not record who
relayed or first-announced the hash.

Shipped collector: [pog-sentry.md](pog-sentry.md).

## Methods

JSON-RPC 2.0. Method names are `pog_` plus the names below. Hex `peerId` values
accept an optional `0x` prefix. Responses omit the prefix.

### `peers`

Connected sessions. No parameters.

| Field | Type | Meaning |
|---|---|---|
| `peerId` | string | 64-byte node public key, hex. Credit key for sends. |
| `enode` | string | `enode://peerId@ip:port` from the live TCP remote, not discv5. |
| `direction` | string | `inbound` or `outbound`. |
| `client` | string | Peer client version. |

### `sendRawTransaction`

Targeted `send_transactions` to one peer.

| Param | Type | Meaning |
|---|---|---|
| `peerId` | string | Must be in the current `peers` set. |
| `rawTx` | string | Signed EIP-2718 bytes, hex. The node does not build or sign. |

Returns `{ txHash, peerId, enode }`. Inserts a RAM row with status `sent`.
Recovers `from` from the payload. Refuses a second send while that signer still
has a `sent` row.

| Error contains | When |
|---|---|
| `syncing` | `nodeStatus.syncing` is true. |
| `not connected` | `peerId` is not in the live session set. |
| `inflight` | That recovered signer already has a `sent` row. |
| `cannot recover signer` | Payload is not a recoverable signature. |
| `invalid peer_id` / `invalid tx hex` / `invalid transaction` | Bad params. |

### `penalize`

Applies stock Reth `ReputationChangeKind::BadProtocol` to `peerId`. Disconnects
now; no redial until `ban_duration` (12 hours by default). Trusted peers are
exempt. The call is not counted on the node and does not require a live session.

| Param | Type | Meaning |
|---|---|---|
| `peerId` | string | Peer to penalize. |

Returns `{ peerId, connected }`. `connected` is whether a session existed at
call time.

### `sends`

RAM send window. No parameters. Keyed by `txHash`. Restart drops the map.

| Field | Type | Meaning |
|---|---|---|
| `txHash` | string | Join key. |
| `peerId` | string | Peer credited at send. |
| `enode` | string | Session enode captured at send. |
| `from` | string | Recovered signer. |
| `nonce` | number | Nonce from the signed tx. |
| `sentAt` | number | Unix seconds at send. |
| `status` | string | `sent`, `landed`, or `timeout`. |
| `blockNumber` | number | Set when `landed`. |

A new-block watcher sets matching hashes to `landed`. A `sent` row becomes
`timeout` after 25 seconds; a later block can still flip it to `landed`. Rows
drop after 15 minutes.

### `nodeStatus`

| Field | Type | Meaning |
|---|---|---|
| `syncing` | boolean | Sends refuse while true. |
| `headNumber` | number | Head block number. |
| `headHash` | string | Head block hash. |
| `sendsInflight` | number | Rows still `sent`. |

## Metrics

Labels are subnet and direction, never `peerId`.

| Series | Labels | Meaning |
|---|---|---|
| `pog_peers` | `subnet`, `direction` | Live sessions per IPv4 `/24` or IPv6 `/48`. |
| `pog_sends_total` | `subnet`, `result` | `sent`, `landed`, `timeout`, `refused`. |
| `pog_sends_inflight` | none | Rows still `sent`. |
| `pog_penalized_total` | `subnet` | `penalize` calls; `unknown` if the peer had already left. |
