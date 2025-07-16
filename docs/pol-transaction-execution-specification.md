# POL Transaction Execution Specification

**Version:** 1.0  
**Authors:** Claude Code Analysis  
**Created:** July 13, 2025  
**Reference:** [BRIP-0004: Enshrined Proof-of-Liquidity (POL) Distributions](https://github.com/berachain/BRIPs/blob/main/meta/BRIP-0004.md)

## Abstract

This specification defines the implementation requirements for Proof of Liquidity (POL) transactions in Ethereum-compatible execution clients. POL transactions enable enshrined validator reward distribution through mandatory system transactions that execute at the beginning of each block after hardfork activation. This document provides comprehensive guidance for implementing POL support in any execution client, including Go-based clients like geth and Rust-based clients like reth.

## Table of Contents

1. [Motivation](#motivation)
2. [Specification Overview](#specification-overview)
3. [Transaction Structure](#transaction-structure)
4. [Execution Semantics](#execution-semantics)
5. [State Changes and Merkle Impacts](#state-changes-and-merkle-impacts)
6. [Hardfork Integration](#hardfork-integration)
7. [Implementation Requirements](#implementation-requirements)
8. [Client-Specific Guidelines](#client-specific-guidelines)
9. [Security Considerations](#security-considerations)
10. [Testing and Validation](#testing-and-validation)

## Motivation

The enshrined POL distribution mechanism addresses critical limitations in the current external function approach:

- **Gas Cost Elimination**: Removes ~1,000,000 gas cost per distribution
- **Real-time Strategy Updates**: Eliminates queue delays for cutting board changes
- **Guaranteed Execution**: Prevents skipped distributions due to high gas prices
- **Enhanced Security**: System-level execution prevents manipulation

POL transactions must be implemented consistently across all execution clients to maintain network consensus and ensure deterministic reward distribution.

## Specification Overview

### Core Architecture

POL transactions use a **synthetic transaction architecture** that balances three requirements:

1. **System Execution**: Zero gas cost with unlimited gas limit
2. **Transparency**: Visible in blocks for indexers and explorers  
3. **Determinism**: Identical execution across all validators

### Key Design Principles

- **Mandatory Inclusion**: POL transactions MUST be included in every block after hardfork activation
- **Index 0 Position**: POL transactions MUST occupy transaction index 0 in block transaction lists
- **Hash-based Validation**: Transaction integrity validated through cryptographic hash comparison
- **Multi-Contract Support**: POL execution MAY interact with multiple smart contracts atomically

## Transaction Structure

### POL Transaction Type

```
Transaction Type: 0x7E (126)
Category: System Transaction
```

### Transaction Fields

```rust
struct PoLTx {
    nonce: u64,           // MUST be block_number - 1
    gas_limit: u64,       // MUST be 0 (system transaction identifier)
    to: Address,          // POL distributor contract address
    value: U256,          // MUST be 0
    input: Bytes,         // ABI-encoded distributeFor(bytes pubkey) call
}
```

### Field Specifications

#### Nonce
- **Value**: `block_number - 1`
- **Purpose**: Unique identification per block using block number
- **Validation**: MUST reject POL transactions with incorrect nonce

#### Gas Limit
- **Value**: `0`
- **Purpose**: Identifies transaction as system-level, bypasses gas validation
- **Execution**: Actual execution uses unlimited gas

#### Target Address
- **Value**: `0x4200000000000000000000000000000000000042` (POL distributor contract)
- **Type**: Fixed system contract address
- **Validation**: MUST reject POL transactions with different target

#### Value
- **Value**: `0`
- **Purpose**: POL distributions don't transfer ETH value
- **Validation**: MUST reject POL transactions with non-zero value

#### Input Data
- **Format**: ABI-encoded function call
- **Function**: `distributeFor(bytes calldata previousProposerPubkey)`
- **Pubkey**: 48-byte BLS public key of previous block proposer
- **Source**: Extracted from consensus layer payload attributes

### Transaction Hash Calculation

```
transaction_hash = keccak256(rlp_encode([
    0x7E,                    // Transaction type prefix
    rlp_encode([nonce, gas_limit, to, value, input])
]))
```

### Serialization Format

#### RLP Encoding
```
POL_TX = [nonce, gas_limit, to, value, input]
TYPED_POL_TX = 0x7E || rlp_encode(POL_TX)
```

#### EIP-2718 Compliance
- Uses EIP-2718 typed transaction format
- Type prefix `0x7E` (126) for POL transactions
- Standard RLP encoding for transaction body

## Execution Semantics

### Execution Model

POL transactions follow a **dual-phase execution model**:

1. **Real Execution Phase**: System call execution during block building
2. **Synthetic Injection Phase**: Transaction added to block for visibility

### Phase 1: Real Execution

#### Pre-conditions
```python
def can_execute_pol(block_timestamp, hardfork_config):
    return hardfork_config.is_prague1_active_at_timestamp(block_timestamp)
```

#### Execution Process
```python
def execute_pol_transaction(evm_state, validator_pubkey):
    # 1. Create POL transaction
    pol_tx = create_pol_transaction(validator_pubkey)
    
    # 2. Execute as system call
    result = evm_state.transact_system_call(
        caller=SYSTEM_ADDRESS,           # 0xfffffffffffffffffffffffffffffffffffffffe
        contract=POL_DISTRIBUTOR_ADDRESS, # 0x4200000000000000000000000000000000000042
        data=pol_tx.input,
        gas_limit=UNLIMITED               # No gas restrictions
    )
    
    # 3. Commit state changes
    evm_state.commit(result.state_changes)
    
    # 4. Generate receipt
    receipt = generate_receipt(pol_tx, result)
    receipts.insert(0, receipt)
    
    return result
```

#### System Call Properties
- **Caller**: `0xfffffffffffffffffffffffffffffffffffffffe` (SYSTEM_ADDRESS)
- **Gas Limit**: Unlimited (bypasses block gas limit)
- **Gas Price**: 0 (no gas cost)
- **Revert Handling**: Silent failure to prevent consensus disruption

### Phase 2: Synthetic Injection

#### Transaction Injection
```python
def inject_pol_transaction(transactions, receipts, hardfork_config, timestamp):
    if hardfork_config.is_prague1_active_at_timestamp(timestamp) and len(receipts) > 0:
        pol_tx = create_pol_transaction(get_validator_pubkey())
        transactions.insert(0, pol_tx)  # Always index 0
    return transactions
```

#### Validation Process
```python
def validate_pol_transaction(received_tx, expected_pubkey):
    # 1. Create canonical POL transaction
    canonical_tx = create_pol_transaction(expected_pubkey)
    
    # 2. Compare transaction hashes
    received_hash = hash_transaction(received_tx)
    canonical_hash = hash_transaction(canonical_tx)
    
    # 3. Validate hash match
    if received_hash != canonical_hash:
        raise InvalidPOLTransaction("Hash mismatch")
    
    # 4. Skip re-execution (already executed in phase 1)
    return ValidationResult.SKIP_EXECUTION
```

### Error Handling

#### Execution Failures
- **Policy**: Silent failure for POL execution errors
- **Logging**: Log failures for debugging but continue block processing
- **Network Impact**: Failed POL execution MUST NOT halt consensus

#### Validation Failures
- **Missing POL**: Block rejection if POL missing after hardfork activation
- **Invalid POL**: Block rejection if POL transaction hash doesn't match canonical
- **Wrong Position**: Block rejection if POL not at index 0

## State Changes and Merkle Impacts

### State Root Calculation

#### Pre-execution State Changes
```python
def apply_pre_execution_changes(state, block):
    # POL execution occurs before user transactions
    if should_execute_pol(block):
        pol_result = execute_pol_transaction(state, block.validator_pubkey)
        state.commit(pol_result.state_changes)
    
    # Apply other pre-execution changes (EIP-4788, etc.)
    apply_beacon_block_root(state, block)
    
    return state
```

#### Multi-Contract State Changes
- POL execution MAY interact with multiple contracts
- All state changes are atomic within single POL execution
- State changes are committed before any user transaction execution

### Transaction Root Impact

#### Merkle Tree Construction
```python
def calculate_transaction_root(transactions):
    # POL transaction is always at index 0 after hardfork
    return merkle_root([
        hash_transaction(tx) for tx in transactions
    ])
```

#### Deterministic Ordering
- POL transactions MUST be placed at index 0
- All other transactions maintain relative ordering
- Transaction root calculation includes POL transaction

### Receipt Root Impact

#### Receipt Generation
```python
def generate_pol_receipt(pol_tx, execution_result):
    return Receipt(
        transaction_hash=hash_transaction(pol_tx),
        transaction_index=0,                    # Always index 0
        cumulative_gas_used=0,                 # System transactions don't consume gas
        status=execution_result.success,
        logs=execution_result.logs
    )
```

#### Receipt Root Calculation
- POL receipt MUST be at index 0 in receipts array
- Cumulative gas used remains 0 for POL receipt
- Receipt root calculation includes POL receipt deterministically

## Hardfork Integration

### Prague1 Hardfork

#### Activation Criteria
```python
class Prague1Hardfork:
    def is_active_at_timestamp(self, timestamp):
        return timestamp >= self.activation_timestamp
    
    def validation_rules(self):
        return [
            "POL transactions required in every block",
            "Minimum base fee of 1 gwei enforced",
            "Base fee parameter changes activated"
        ]
```

#### Pre-hardfork Behavior
- POL transactions MUST be rejected
- Blocks MUST NOT contain POL transactions
- Normal transaction processing without POL

#### Post-hardfork Behavior
- POL transactions MUST be included in every block
- Blocks without POL transactions MUST be rejected
- POL validation rules fully active

### Consensus Layer Integration

#### Payload Attributes Enhancement
```python
class BerachainPayloadAttributes:
    # Standard Ethereum fields
    timestamp: int
    prev_randao: bytes32
    suggested_fee_recipient: address
    
    # Berachain-specific fields
    previous_proposer_pubkey: bytes48  # Required post-Prague1
```

#### Validation Requirements
- Consensus layer MUST provide `previous_proposer_pubkey` in all payload attributes after Prague1
- Execution layer MUST reject payloads with missing pubkey post-hardfork
- Pubkey format MUST be valid 48-byte BLS public key

## Implementation Requirements

### Core Components

#### 1. Transaction Type Registration
```python
# Add POL transaction type to client transaction types
TRANSACTION_TYPES = {
    0x00: LegacyTransaction,
    0x01: AccessListTransaction,
    0x02: FeeMarketTransaction,
    0x03: BlobTransaction,
    0x7E: POLTransaction,        # New POL transaction type
}
```

#### 2. Transaction Pool Modifications
```python
def validate_transaction_for_pool(tx):
    if tx.type == 0x7E:  # POL transaction
        # POL transactions should not enter mempool
        raise RejectTransaction("POL transactions are system-generated only")
    
    return standard_validation(tx)
```

#### 3. Block Building Integration
```python
def build_block(parent, payload_attributes, transactions):
    state = load_state(parent.state_root)
    
    # Apply pre-execution changes including POL
    apply_pre_execution_changes(state, payload_attributes)
    
    # Execute user transactions
    receipts = []
    for tx in transactions:
        receipt = execute_transaction(state, tx)
        receipts.append(receipt)
    
    # Inject POL transaction for visibility
    if should_inject_pol(payload_attributes.timestamp):
        pol_tx = create_pol_transaction(payload_attributes.previous_proposer_pubkey)
        transactions.insert(0, pol_tx)
    
    return build_block_from_state(state, transactions, receipts)
```

#### 4. Block Validation Integration
```python
def validate_block(block):
    # Standard Ethereum validation
    validate_header(block.header)
    validate_transactions(block.transactions)
    
    # POL-specific validation
    if is_prague1_active(block.timestamp):
        validate_pol_transaction(block.transactions[0], block.header.timestamp)
    
    return ValidationResult.VALID
```

### System Call Infrastructure

#### EVM Integration
```python
def transact_system_call(evm, caller, contract, data):
    # Create system transaction context
    tx_context = TransactionContext(
        origin=caller,
        gas_price=0,
        gas_limit=UNLIMITED
    )
    
    # Execute with system privileges
    result = evm.execute(
        caller=caller,
        contract=contract,
        input=data,
        context=tx_context,
        is_static=False
    )
    
    return result
```

## Client-Specific Guidelines

### Go-based Clients (geth-style)

#### File Modifications Required

1. **Transaction Types** (`core/types/transaction.go`)
```go
const (
    LegacyTxType = iota
    AccessListTxType
    DynamicFeeTxType
    BlobTxType
    POLTxType = 0x7E  // Add POL transaction type
)
```

2. **State Processor** (`core/state_processor.go`)
```go
func (p *StateProcessor) Process(block *types.Block, statedb *state.StateDB, cfg vm.Config) (*state.StateDB, types.Receipts, *big.Int, error) {
    // Apply pre-execution changes
    if err := p.ProcessPOLTransaction(statedb, block.Header()); err != nil {
        return nil, nil, nil, err
    }
    
    // Process regular transactions
    return p.processTransactions(block, statedb, cfg)
}
```

3. **Transaction Pool** (`core/txpool/validation.go`)
```go
func (v *Validator) ValidateTransaction(tx *types.Transaction) error {
    if tx.Type() == types.POLTxType {
        return errors.New("POL transactions not allowed in mempool")
    }
    return v.validateStandardTransaction(tx)
}
```

### Rust-based Clients (reth-style)

#### Trait Implementations
```rust
impl Transaction for PoLTx {
    fn chain_id(&self) -> Option<ChainId> { None }
    fn nonce(&self) -> u64 { self.nonce }
    fn gas_limit(&self) -> u64 { self.gas_limit }
    fn gas_price(&self) -> Option<u128> { Some(0) }
    fn max_fee_per_gas(&self) -> u128 { 0 }
    fn max_priority_fee_per_gas(&self) -> Option<u128> { Some(0) }
    fn to(&self) -> TxKind { TxKind::Call(self.to) }
    fn value(&self) -> U256 { self.value }
    fn input(&self) -> &Bytes { &self.input }
}
```

#### Execution Integration
```rust
impl BlockExecutor for BerachainExecutor {
    fn execute_pre_execution_changes(&mut self, header: &Header) -> Result<()> {
        if self.chain_spec.is_prague1_active_at_timestamp(header.timestamp) {
            self.execute_pol_transaction(header)?;
        }
        Ok(())
    }
}
```

### WebAssembly Clients

#### Interface Bindings
```typescript
interface POLTransaction {
    type: 0x7E;
    nonce: number; // block_number - 1
    gasLimit: 0;
    to: '0x4200000000000000000000000000000000000042';
    value: 0;
    input: Uint8Array; // ABI-encoded distributeFor call
}
```

## Security Considerations

### Attack Vectors and Mitigations

#### 1. POL Omission Attacks
**Attack**: Malicious validators omit POL transactions to avoid reward distribution.
**Mitigation**: 
- Transaction root mismatch detection
- Block rejection for missing POL transactions
- Network-level consensus enforcement

#### 2. POL Modification Attacks
**Attack**: Attackers modify POL transaction fields to redirect rewards.
**Mitigation**:
- Cryptographic hash validation
- Deterministic transaction generation
- Multi-layer validation (hardfork, hash, merkle)

#### 3. Execution Bypass Attacks
**Attack**: Malicious clients accept POL transactions without executing them.
**Mitigation**:
- State root verification ensures execution occurred
- Receipt validation confirms proper execution
- Network consensus on state changes

#### 4. Validator Pubkey Manipulation
**Attack**: Incorrect validator pubkeys in payload attributes.
**Mitigation**:
- Consensus layer validation of pubkey source
- Cryptographic validation of pubkey format
- Trust model: execution trusts consensus layer data

### Cryptographic Properties

#### Transaction Integrity
- Hash-based validation provides tamper detection
- Deterministic construction ensures consistency
- EIP-2718 compliance maintains standard security properties

#### State Commitment Security
- Merkle tree inclusion proofs for state changes
- Atomic commitment of multi-contract interactions
- Cryptographic binding of POL execution to block state

### Byzantine Fault Tolerance

#### Honest Majority Assumptions
- Network requires >2/3 honest validators for safety
- POL transactions inherit network's security assumptions
- Malicious minority cannot compromise POL execution

#### Consensus Failure Modes
- POL execution failures are isolated (silent failure)
- Network continues operation despite individual POL failures
- Failed POL execution logged but doesn't halt consensus

## Testing and Validation

### Test Vector Generation

#### Canonical POL Transaction
```json
{
    "type": "0x7E",
    "nonce": "0x9",     // block_number - 1 (example: block 10)
    "gasLimit": "0x0",
    "to": "0x4200000000000000000000000000000000000042",
    "value": "0x0",
    "input": "0x...",  // ABI-encoded distributeFor call
    "hash": "0x...",   // Expected transaction hash
}
```

#### Block Construction Test
```python
def test_pol_block_construction():
    # Create block with POL transaction
    block = create_test_block_with_pol()
    
    # Validate POL at index 0
    assert block.transactions[0].type == 0x7E
    assert block.transactions[0].gas_limit == 0
    assert block.transactions[0].nonce == block.number - 1
    
    # Validate merkle roots
    assert block.header.transactions_root == calculate_transaction_root(block.transactions)
    assert block.header.receipts_root == calculate_receipt_root(block.receipts)
```

### Integration Test Scenarios

#### 1. Hardfork Transition Testing
- Pre-Prague1: Reject POL transactions
- Prague1 activation: Accept and require POL transactions
- Post-Prague1: Enforce POL transaction presence

#### 2. Multi-Client Compatibility
- Generate identical POL transactions across different clients
- Verify consistent state root calculations
- Test block propagation and validation

#### 3. Error Handling Testing
- Test POL execution failures (contract revert)
- Test missing POL transactions (block rejection)
- Test invalid POL transactions (validation failure)

#### 4. Performance Testing
- Measure POL execution overhead
- Test block building and validation performance
- Verify gas accounting accuracy

### Compliance Validation

#### Reference Implementation Tests
```bash
# Test POL transaction creation
test_pol_transaction_creation()

# Test hardfork activation
test_prague1_hardfork_activation()

# Test block building with POL
test_block_building_with_pol()

# Test block validation with POL
test_block_validation_with_pol()

# Test multi-contract POL execution
test_pol_multi_contract_execution()
```

## Conclusion

This specification provides comprehensive guidance for implementing POL transaction support in any Ethereum-compatible execution client. The synthetic transaction architecture successfully balances system execution requirements with transparency and validation needs.

Key implementation takeaways:

1. **Dual-phase execution** ensures both system privileges and block visibility
2. **Hash-based validation** provides efficient and secure transaction verification
3. **Deterministic generation** maintains consensus across diverse client implementations
4. **Hardfork gating** enables clean activation and backward compatibility
5. **Multi-contract support** allows complex reward distribution logic

Successful implementation of this specification will enable enshrined POL distributions that eliminate gas costs, enable real-time strategy updates, and provide guaranteed execution for validator rewards in the Berachain ecosystem.

## References

- [BRIP-0004: Enshrined Proof-of-Liquidity (POL) Distributions](https://github.com/berachain/BRIPs/blob/main/meta/BRIP-0004.md)
- [EIP-2718: Typed Transaction Envelope](https://eips.ethereum.org/EIPS/eip-2718)
- [EIP-4788: Beacon block root in the EVM](https://eips.ethereum.org/EIPS/eip-4788)
- [Ethereum Yellow Paper: Formal Specification](https://ethereum.github.io/yellowpaper/paper.pdf)
- [bera-reth POL Transaction Architecture](./pol-transaction-architecture.md)

---

**Specification Version:** 1.0  
**Last Updated:** July 13, 2025  
**Next Review:** After Prague1 hardfork activation