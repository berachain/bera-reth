# Bera-Reth

A Rust execution client for Berachain, built with the Reth SDK.

⚠️ **Not ready for production**

## Getting Started

### Prerequisites

- Rust 1.70+
- Git

### Build and Run

```bash
git clone https://github.com/berachain/bera-reth.git
cd bera-reth
cargo build --release
```

### Local Testing with BeaconKit

```bash
BEACON_KIT_PATH=/path/to/beacon-kit ./scripts/test-block-progression.sh
```

## Development

### Quality Checks

```bash
# Run all checks before submitting PRs
make pr

# Auto-fix formatting
make pr-fix
```

## License

Apache-2.0
