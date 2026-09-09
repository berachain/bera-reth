# Sentry-free Proof of Gossip

Proof of Gossip (PoG) tests targeted transaction delivery and later canonical-chain
inclusion. It does not prove that the target peer relayed the transaction.

The node exposes a small IPC-only API. A downstream collector constructs and signs
canaries, manages nonces, and stores durable results. The node sends each signed
transaction to one connected peer, tracks the send in memory, and publishes
low-cardinality network observations.

## Enable PoG

Start the node with:

```bash
bera-reth node --bera.pog
```

The flag installs the `pog` namespace on IPC only. It does not expose PoG through
HTTP or WebSocket.

## IPC API

### `pog_peers`

Returns connected peers with:

- `peerId`: the stable 64-byte node public key used for credit.
- `enode`: `enode://peerId@ip:port` built from the live session TCP endpoint.
- `direction`: `inbound` or `outbound`.
- `client`: the peer client version.

The session endpoint is authoritative for the current connection. The advertised
discovery endpoint can differ from the established remote address.

### `pog_sendRawTransaction(peerId, rawTx)`

Accepts a signed EIP-2718 transaction and sends it only to the named connected
peer. The node recovers the signer and refuses the request when:

- the node is syncing;
- the target peer is not connected; or
- that signer already has an inflight PoG send.

On success, the response contains `txHash`, `peerId`, and the session `enode`
captured at send time. More signer accounts provide parallelism. The node does not
reserve nonces or construct transactions.

### `pog_sends`

Returns the in-memory send window:

- `txHash`, `peerId`, and the captured session `enode`;
- recovered `from` and signed `nonce`;
- `sentAt`;
- `status`: `sent`, `landed`, or `timeout`;
- `blockNumber` when landed.

A canonical-block watcher marks matching hashes as landed. A send times out after
25 seconds, but a later canonical match can still change it to landed. Rows expire
after 15 minutes. Restarting the node discards unresolved rows, so the collector
must retain durable history.

### `pog_nodeStatus`

Returns `syncing`, `headNumber`, `headHash`, and `sendsInflight`.

## Prometheus metrics

PoG uses subnet labels to keep metric cardinality bounded:

- `pog_peers{subnet,direction}`: current connected peers per IPv4 `/24` or IPv6
  `/48`.
- `pog_sends_total{subnet,result}`: `sent`, `landed`, `timeout`, and `refused`
  outcomes.
- `pog_sends_inflight`: current in-memory sends awaiting inclusion or timeout.

Peer IDs and individual IP addresses never appear as metric labels. Collectors
join subnet observations to `pog_peers` and `pog_sends` when peer-level attribution
is required.

## Claim boundary

PoG establishes: the node sent transaction hash `H` exclusively to peer `P`, then
`H` appeared on the canonical chain.

PoG does not establish that `P` relayed `H`, nor that `P` was the first peer to
announce it. Those claims require transaction provenance that stock Reth does not
provide.
