<div align="center">

<img src="assets/bera-reth.png" alt="Bera-Reth" width="400"/>

<p>
  <a href="https://github.com/berachain/bera-reth/actions/workflows/ci.yml">
    <img src="https://github.com/berachain/bera-reth/actions/workflows/ci.yml/badge.svg" alt="CI"/>
  </a>
  <a href="https://github.com/berachain/bera-reth">
    <img src="https://img.shields.io/badge/status-production-brightgreen" alt="Status"/>
  </a>
</p>

</div>

# Bera-Reth

A high-performance Rust execution client for Berachain, built with the Reth SDK (pinned to reth `v2.5.0`).

## Getting Started

### Prerequisites

- Rust 1.95+ (MSRV inherited from reth v2.5.0; current stable works)
- A nightly toolchain for `make pr` (rustfmt/clippy)
- Git

### Build and Run

```bash
git clone https://github.com/berachain/bera-reth.git
cd bera-reth
cargo build --release
```

## Running with BeaconKit

Bera-reth is the execution layer; [BeaconKit](https://github.com/berachain/beacon-kit) is the consensus layer that drives it over the Engine API. The authoritative version-pairing table per network lives at [docs.berachain.com/nodes/architecture/evm-execution](https://docs.berachain.com/nodes/architecture/evm-execution).

For local development, clone BeaconKit next to this repository:

```bash
git clone https://github.com/berachain/beacon-kit.git ../beacon-kit
```

Two-terminal flow:

```bash
# Terminal 1 — consensus client; also generates the EL genesis at
# .tmp/beacond/eth-genesis.json (including EIP-6110 deposit storage)
cd ../beacon-kit && make start

# Terminal 2 — execution client, built and launched against that genesis
BEACON_KIT=../beacon-kit make start-bera-reth-local
```

Or run the one-shot integration test that launches both and monitors block progression:

```bash
BEACON_KIT_PATH=../beacon-kit ./scripts/test-block-progression.sh
```

The flags that matter when pairing with BeaconKit:

- `--chain <eth-genesis.json>` — must be passed on every start; BeaconKit generates this file.
- `--authrpc.jwtsecret <jwt.hex>` — must point at the same file as beacond's `--beacon-kit.engine.jwt-secret-path`.
- `--engine.persistence-threshold 0` and `--engine.memory-block-buffer-target 0` — BeaconKit finalizes every block, so in-memory block buffering must be off.

Ports: `8545` (HTTP RPC), `8551` (Engine API), `30303` (P2P), and `3500` (beacond node API) must be free.

## Storage

Reth v2 introduces a hot/cold storage layout ("Storage V2"): history indices and transaction lookups move to RocksDB, historical changesets move to static files, and hashed state stays on MDBX. Fresh datadirs use V2 automatically; existing V1 datadirs keep working unchanged. To migrate an existing node in place, stop it and run:

```bash
bera-reth db --datadir <datadir> migrate-v2 --chain <genesis.json>
```

See [docs/storage-v2.md](docs/storage-v2.md) for the full operator guide.

## Development

### Prerequisites

Install required development tools:

```bash
# Install dprint for TOML formatting
curl -fsSL https://dprint.dev/install.sh | sh

# Install cargo-deny for dependency auditing
cargo install cargo-deny
```

### Quality Checks

```bash
# Run all checks before submitting PRs
make pr

# Auto-fix formatting
make pr-fix
```

## License

Apache-2.0
