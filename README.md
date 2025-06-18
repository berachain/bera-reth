<div align="center">

<img src="assets/bera-reth.png" alt="Logo" width="250"/>

<p>
  <a href="https://github.com/berachain/bera-reth/actions/workflows/ci.yml">
    <img src="https://github.com/berachain/bera-reth/actions/workflows/ci.yml/badge.svg" alt="CI"/>
  </a>
  <a href="https://github.com/berachain/bera-reth">
    <img src="https://img.shields.io/badge/status-in%20development-yellow.svg" alt="Status"/>
  </a>
</p>

</div>

# 🐻⛓️ Bera-Reth: A high-performance Rust Execution Client for Berachain, powered by Reth SDK 🐻⛓️

---

## ⚠️ WARNING: This is not ready for production ⚠️

## 🚀 Quickstart

### Prerequisites

- **Rust** (≥ 1.70) with components:
  ```bash
  rustup component add rustfmt clippy
  cargo install cargo-audit cargo-udeps
  ```
- **GNU Make** (optional, for helper make targets)
- **Git**

```bash
git clone https://github.com/berachain/bera-reth.git
cd bera-reth
```

---

## 📦 Building

```bash
# Debug build
cargo build

# Optimized release build
cargo build --release
```

The binary will be at `target/release/bera-reth`.

---

## ▶️ Running with BeaconKit (Local Development)

For local development and testing, you can use the provided script to test BeaconKit integration:

```bash
# Basic usage (tests progression to block 10 with 120s timeout)
BEACON_KIT_PATH=/path/to/beacon-kit ./scripts/test-block-progression.sh

# Custom configuration
BEACON_KIT_PATH=/path/to/beacon-kit TARGET_BLOCK=5 TIMEOUT=180 ./scripts/test-block-progression.sh
```

### Prerequisites

- Local BeaconKit repository cloned and built
- Set `BEACON_KIT_PATH` to your BeaconKit directory

### Environment Variables

- `BEACON_KIT_PATH`: Path to your BeaconKit repository (required)
- `TARGET_BLOCK`: Target block number to reach (default: `10`)
- `TIMEOUT`: Maximum time to wait in seconds (default: `120`)

### What the script does

1. Cleans up any existing data directories
2. Starts BeaconKit locally with `[BEACONKIT]` log prefixes
3. Starts bera-reth with `[RETH]` log prefixes
4. Monitors block progression via JSON-RPC calls
5. Reports success when target block is reached
6. Automatically cleans up all processes on exit or Ctrl+C

### Manual Setup (Alternative)

If you prefer to run the components manually:

1. Run `make start` from **your BeaconKit repository**
2. Run `BEACON_KIT=/path/to/beacon-kit make start-bera-reth-local` from **this repository**

---

## 🔧 Testing & Quality

We enforce formatting, linting, security, and dead-code checks:

```bash
# 1️⃣ Check formatting
cargo fmt --all -- --check

# 2️⃣ Lint with Clippy (deny all warnings)
cargo clippy --all-targets --all-features -- -D warnings

# 3️⃣ Run tests
cargo test --all --locked --verbose

# 4️⃣ Security audit
cargo audit

# 5️⃣ Detect unused dependencies
cargo udeps --all-features --locked
```

---

## 📚 Documentation

View the comprehensive code documentation locally:

```bash
# Build and open documentation in your browser
cargo doc --open --no-deps --document-private-items
```

This will generate and open detailed API documentation including all modules, types, and examples.

## 📜 License

Licensed under the Apache-2.0 License. See [LICENSE](LICENSE) for details.
