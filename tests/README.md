# Bera-Reth Integration Tests

This directory contains end-to-end integration tests for Bera-Reth, following Reth's established testing patterns.

## Structure

```
tests/
├── README.md           # This file
└── e2e/               # End-to-end integration tests
    ├── mod.rs         # Test setup and utilities
    ├── pol_transactions.rs  # PoL transaction testing
    └── rpc_integration.rs   # RPC endpoint testing
```

## Running Tests

```bash
# Run all e2e tests
cargo test --test e2e

# Run specific test modules
cargo test --test e2e pol_transactions
cargo test --test e2e rpc_integration

# Run with output (useful for debugging)
cargo test --test e2e -- --nocapture

# Run specific test
cargo test --test e2e test_pol_transaction_current_behavior -- --nocapture
```

## Test Categories

### PoL Transaction Tests (`pol_transactions.rs`)
- Tests current PoL transaction behavior via RPC
- Verifies Ethereum transaction acceptance
- Examines block production and structure
- Multi-node consensus testing (when enabled)

### RPC Integration Tests (`rpc_integration.rs`)
- Basic RPC method functionality
- Transaction submission and mining
- Error handling and validation
- Concurrent transaction processing
- Berachain-specific features

## Key Features

- **Real Node Testing**: Uses `NodeTestContext` for full integration testing
- **Actual RPC Servers**: Tests HTTP endpoints with real JSON-RPC calls
- **Chain State Validation**: Verifies blockchain state changes
- **Berachain-Specific**: Tests Prague1 hardfork and PoL transaction behavior
- **Performance Testing**: Concurrent transaction submission and validation

## Test Utilities

Common test setup functions are in `mod.rs`:
- `berachain_test_setup()` - Single node test configuration
- `berachain_multi_node_setup(n)` - Multi-node network configuration  
- `berachain_test_chain_spec()` - Berachain test chain specification

## Development Notes

- Tests establish baseline behavior for PoL transactions
- Can be extended to test rejection logic when implemented
- Some tests are marked `#[ignore]` for resource-intensive scenarios
- WebSocket tests are placeholders pending WS implementation

For detailed guidance on writing e2e tests, see `docs/e2e-testing-guide.md`.