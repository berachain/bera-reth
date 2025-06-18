# Bera-Reth Development Guide for AI Agents

Bera-Reth is a high-performance Rust execution client for Berachain, built on the Reth SDK. This guide helps AI agents understand the project structure, development patterns, and contribution practices.

## Project Overview

**What**: Rust execution client for Berachain using Reth SDK  
**Status**: In development, not production ready  
**Architecture**: Standard Ethereum execution with Berachain-specific chain configuration  
**Key Difference**: Prague1 hardfork activated at genesis (time: 0) for 1 gwei minimum base fee

## Core Architecture

### Key Components
- **Chain Specification**: `src/chainspec/mod.rs` - Berachain network parameters
- **Genesis Configuration**: `src/genesis/mod.rs:53` - Prague1 fork timing
- **Hardforks**: `src/hardforks/mod.rs` - Prague1 activation logic
- **Node Builder**: `src/node/mod.rs` - Reth SDK integration
- **CLI**: `src/node/cli.rs` - Command-line interface

### Reference Implementations
Users should clone these repositories alongside bera-reth for development reference:

- **Prime Reference**: [Reth](https://github.com/paradigmxyz/reth) - Study `src/main.rs` and node builder patterns
- **Inspiration**: [Reth-BSC](https://github.com/paradigmxyz/reth/tree/main/examples/bsc) - Chain-specific adaptations  
- **Integration**: [BeaconKit](https://github.com/berachain/beacon-kit) - Required for local testing

Recommended directory structure:
```
Code/
├── bera-reth/          # This repository
├── reth/               # git clone https://github.com/paradigmxyz/reth.git
├── reth-bsc/           # git clone https://github.com/paradigmxyz/reth.git reth-bsc && cd reth-bsc && git checkout examples/bsc
└── beacon-kit/         # git clone https://github.com/berachain/beacon-kit.git
```

## Development Workflow

### Build Commands
```bash
# Standard build (not make build)
cargo build --release

# Integration testing with BeaconKit
BEACON_KIT_PATH=../beacon-kit make start-bera-reth-local

# Local development testing
./scripts/test-block-progression.sh
```

### Code Quality
```bash
# Run all quality checks (REQUIRED before every commit)
make pr

# Auto-fix formatting issues
make pr-fix
```

### Testing Strategy
- **Integration**: BeaconKit + bera-reth via JSON-RPC monitoring
- **Local Only**: No CI BeaconKit integration (removed by design)
- **Script**: `./scripts/test-block-progression.sh` with debug mode (`set -ex`)
- **Target**: Block progression monitoring to configurable target

## Contribution Patterns

### Code Style Preferences
- **No comments** unless explicitly requested
- **Custom error types** over generic errors
- **Succinct documentation** focusing on "why"
- **Extract magic numbers** into documented constants
- **Follow Reth patterns** from reference implementations

### Communication Guidelines
- **All code and PR comments must be succinct**
- **NEVER reference AI tools in commits, PRs, or code**
- **Focus on answering exactly what was asked**
- **Keep responses concise and direct**

### Git Workflow
- **Main branch**: `main` (no `develop` branch)
- **Pre-commit requirement**: ALWAYS run `make pr` before committing
- **Commit style**: Follow semantic format (feat:, fix:, perf:, chore:) with imperative mood under 50 characters
- **PR descriptions**: Use flowing prose to explain what changed and why, providing context for reviewers
- **AI tool references**: STRICTLY FORBIDDEN in all commits, PRs, and code
- **Guidelines**: Reference https://github.com/ithacaxyz/claude-md/blob/main/CLAUDE.md for best practices on technical writing and documentation structure

## Key File Locations

### Configuration
- `src/genesis/mod.rs:53` - Prague1 genesis configuration
- `src/chainspec/mod.rs` - Berachain network parameters
- `src/hardforks/mod.rs` - Hardfork activation logic

### Testing
- `scripts/test-block-progression.sh` - Integration test script
- Log prefixes: `[BEACONKIT]` and `[RETH]` for process identification

### Build System
- `Cargo.toml` - Rust dependencies and workspace
- `Makefile` - Helper targets for local development

## Common Tasks

### Adding New Features
1. Study `../reth/src/main.rs` implementation patterns first
2. Follow existing patterns in bera-reth codebase
3. Extract constants for magic numbers
4. Write succinct documentation
5. Test with `../beacon-kit` integration locally

### Debugging Integration
- Use script debug mode: `set -ex` in test script
- Monitor both BeaconKit and Reth logs with prefixes
- Check genesis file generation: `.tmp/beacond/eth-genesis.json`
- Verify port availability: 8545, 8551, 30303, 3500

### Performance Optimization
- Focus on Reth SDK optimization patterns
- Reference `../reth` implementation for best practices
- Consider Berachain-specific optimizations
- Benchmark against standard Ethereum clients

## Quick Reference

### Essential Commands
```bash
# Build
cargo build --release

# Test locally
BEACON_KIT_PATH=/path/to/beacon-kit ./scripts/test-block-progression.sh

# Quality checks (run before EVERY commit)
make pr

# Auto-fix formatting
make pr-fix

# Documentation
cargo doc --open --no-deps --document-private-items
```

### Key Environment Variables
- `BEACON_KIT_PATH`: Path to BeaconKit repository (required for testing)
- `TARGET_BLOCK`: Block number target for testing (default: 10)
- `TIMEOUT`: Test timeout in seconds (default: 120)

## Repository Structure

```
bera-reth/
├── src/
│   ├── chainspec/     # Berachain network configuration
│   ├── genesis/       # Genesis block and Prague1 setup
│   ├── hardforks/     # Hardfork activation logic
│   ├── node/          # Node builder and CLI
│   └── main.rs        # Entry point
├── scripts/
│   └── test-block-progression.sh  # Integration testing
├── Cargo.toml         # Dependencies and workspace
└── README.md          # User documentation
```

This structure follows Reth SDK patterns while adding Berachain-specific configurations and integration testing capabilities.