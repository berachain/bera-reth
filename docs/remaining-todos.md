# Remaining TODOs for Production Readiness

This document tracks the remaining work needed to make bera-reth production-ready.

## High Priority - User Requested

### Validator Pubkey Integration
**Location**: `src/node/evm/executor.rs:87-88`
**Issue**: Currently using hardcoded zero value for validator pubkey in POL transactions
**Required**: Update to source validator pubkey from payload attributes instead of hardcoded bytes
```rust
// Current implementation uses zero value:
let pubkey = vec![0u8; 48]; // TODO: Get from payload attributes
```

### API Implementation
**Location**: `src/rpc/api.rs`
**Issue**: Multiple `todo!()` implementations in RPC transaction builder
**Required**: Complete RPC implementation for transaction handling

### Receipt Builder
**Issue**: Need to fix receipt builder to use new generic receipt types
**Required**: Update receipt building to support Berachain transaction types properly

## Critical Implementation Gaps

### POL Transaction Hash Validation
**Status**: ✅ RESOLVED
**Fixed**: Changed from `Sealed::new_unchecked(pol_tx, B256::ZERO)` to `Sealed::new(pol_tx)` for proper hash calculation

### Multi-Contract State Persistence
**Status**: ✅ RESOLVED  
**Fixed**: Removed alloy-evm state retention filter via local fork to support POL multi-contract interactions

### Transaction Type Integration
**Location**: `src/transaction/mod.rs`
**Issue**: Incomplete BerachainTxEnvelope implementation
**Requirements**:
- Complete envelope type conversion methods
- Fix pooled transaction handling
- Implement proper type coercion between Ethereum and Berachain transaction types

### Engine Integration
**Location**: `src/engine/payload.rs`
**Issue**: Incomplete payload attributes handling
**Requirements**:
- Complete BerachainPayloadAttributes implementation
- Add proper validator pubkey extraction from payload
- Ensure engine API compatibility

### Node Primitives
**Location**: `src/primitives/mod.rs`
**Issue**: Receipt type may need updating for generic receipts
**Current**: Uses `reth_ethereum_primitives::Receipt`
**Consideration**: May need Berachain-specific receipt type

## Medium Priority Enhancements

### Error Handling
**Locations**: Various `todo!()` and `unimplemented!()` calls throughout codebase
**Requirements**:
- Replace placeholder error handling with proper Berachain-specific errors
- Add comprehensive error context for debugging
- Implement proper error propagation chains

### Transaction Pool Integration
**Location**: `src/pool/transaction.rs:133`
**Issue**: `From<Recovered<BerachainPooledTransactionVariant>>` implementation incomplete
**Required**: Complete transaction pool type conversions

### Payload Validation
**Location**: `src/engine/validator.rs`
**Enhancement**: Add Berachain-specific payload validation rules
**Current**: Uses standard Ethereum validation

### Network Configuration
**Enhancement**: Add Berachain-specific network parameters and discovery
**Consider**: Bootnode configuration, network protocols

## Low Priority Optimizations

### Performance Monitoring
**Enhancement**: Add Berachain-specific metrics and telemetry
**Consider**: POL transaction success rates, multi-contract state change metrics

### Documentation
**Enhancement**: Complete inline documentation for all public APIs
**Note**: Following codebase preference for minimal comments unless requested

### Testing Coverage
**Enhancement**: Expand unit test coverage for Berachain-specific components
**Current**: Basic integration testing with BeaconKit

## Verification Requirements

### Hardfork Gating
**Status**: ✅ VERIFIED
**Confirmed**: All POL logic properly gated behind Prague1 hardfork checks in both executor and assembler

### Chain Specification
**Status**: ✅ VERIFIED  
**Confirmed**: Prague1 hardfork properly integrated into chain spec with base fee enforcement

### Integration Testing
**Status**: ✅ WORKING
**Confirmed**: Block progression testing with BeaconKit via `./scripts/test-block-progression.sh`

## Completion Checklist

- [ ] Update validator pubkey sourcing from payload attributes
- [ ] Complete RPC API implementation (remove `todo!()` calls)
- [ ] Fix receipt builder for generic receipt types
- [ ] Complete transaction envelope type conversions
- [ ] Implement proper error handling throughout
- [ ] Add comprehensive payload validation
- [ ] Expand test coverage for edge cases
- [ ] Performance benchmarking and optimization
- [ ] Security audit preparation
- [ ] Production deployment configuration

## Notes

- POL transaction implementation is functionally complete with working multi-contract state changes
- Alloy-evm patch successfully enables POL system calls across multiple contracts
- Prague1 hardfork gating ensures compatibility with Ethereum networks
- Current implementation handles 1 gwei minimum base fee enforcement correctly