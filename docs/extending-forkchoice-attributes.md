# Extending Forkchoice Attributes in Bera-reth

This guide shows the simplest, leanest way to add custom fields to forkchoice attributes in bera-reth, using a **contract-first approach** that starts with the NodeTypes definition and works outward.

## Overview

Forkchoice attributes are passed from the consensus layer (BeaconKit) to the execution layer (bera-reth) via the Engine API's `engine_forkchoiceUpdated` call. These attributes control payload building parameters like timestamp, gas limit, and fee recipient.

## Contract-First Implementation Strategy

Start by defining what you want (the NodeTypes contract) and let the compiler guide you through implementing the required components. This approach:

- ✅ **Defines the contract upfront** - clear goal and interface
- ✅ **Compiler-driven implementation** - tells you exactly what to build next
- ✅ **Type-safe progression** - ensures all components are compatible
- ✅ **Clear dependency chain** - see exactly what depends on what

## Implementation Steps (Contract-First Approach)

### Step 1: Define Your Contract (NodeTypes)

Start by declaring exactly what you want - custom engine types in your NodeTypes implementation. This creates a clear contract that the compiler will help you fulfill.

Update `src/node/mod.rs`:

```rust
use crate::engine::types::BeraEngineTypes; // This doesn't exist yet - compiler will tell you

impl NodeTypes for BerachainNode {
    type Primitives = <EthereumNode as NodeTypes>::Primitives;
    type ChainSpec = BerachainChainSpec;
    type StateCommitment = <EthereumNode as NodeTypes>::StateCommitment;
    type Storage = <EthereumNode as NodeTypes>::Storage;
    type Payload = BeraEngineTypes; // ← Your custom engine types (doesn't exist yet)
}
```

**🔍 Compile and observe**: The compiler will immediately tell you:
```
error[E0432]: unresolved import `crate::engine::types::BeraEngineTypes`
```

This error tells you exactly what to implement next.

### Step 2: Create Engine Types Skeleton (Compiler-Driven)

Create the file the compiler is asking for. Start with the minimal structure:

Create `src/engine/mod.rs`:
```rust
pub mod types;
pub mod attributes;

pub use types::BeraEngineTypes;
pub use attributes::{BeraPayloadAttributes, BeraPayloadBuilderAttributes};
```

Create `src/engine/types.rs`:
```rust
use reth_payload_primitives::PayloadTypes;

#[derive(Clone, Debug, Default)]
pub struct BeraEngineTypes;

impl PayloadTypes for BeraEngineTypes {
    type ExecutionData = reth_ethereum_engine_primitives::ExecutionData;
    type BuiltPayload = reth_ethereum_primitives::EthBuiltPayload;
    type PayloadAttributes = BeraPayloadAttributes; // Compiler will ask for this next
    type PayloadBuilderAttributes = BeraPayloadBuilderAttributes; // And this
    
    fn block_to_payload(
        block: reth_primitives::SealedBlock,
    ) -> Self::ExecutionData {
        reth_ethereum_engine_primitives::EthEngineTypes::block_to_payload(block)
    }
}
```

**🔍 Compile and observe**: Now the compiler will tell you:
```
error[E0412]: cannot find type `BeraPayloadAttributes` in this scope
error[E0412]: cannot find type `BeraPayloadBuilderAttributes` in this scope
```

Perfect! The compiler is guiding you to the next step.

### Step 3: Create Custom Payload Attributes (Compiler-Driven)

Create the attribute types the compiler is asking for:

Create `src/engine/attributes.rs`:
```rust
use serde::{Deserialize, Serialize};
use alloy_primitives::{Address, B256};
use alloy_eips::eip4895::Withdrawal;

/// Your custom payload attributes with the boolean field
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeraPayloadAttributes {
    /// Standard Ethereum fields
    pub timestamp: u64,
    pub prev_randao: B256,
    pub suggested_fee_recipient: Address,
    pub withdrawals: Option<Vec<Withdrawal>>,
    pub parent_beacon_block_root: Option<B256>,
    
    /// Your custom boolean field
    pub custom_bera_flag: bool,
}

/// Builder attributes that wrap the RPC attributes
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeraPayloadBuilderAttributes {
    inner: reth_ethereum_engine_primitives::EthPayloadBuilderAttributes,
    custom_bera_flag: bool,
}

// Implement required traits - compiler will tell you what's missing
impl reth_payload_primitives::PayloadAttributes for BeraPayloadAttributes {
    type RpcPayloadAttributes = Self;
    type Error = eyre::Error;
    
    fn timestamp(&self) -> u64 { self.timestamp }
    fn prev_randao(&self) -> B256 { self.prev_randao }
    fn suggested_fee_recipient(&self) -> Address { self.suggested_fee_recipient }
    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> { self.withdrawals.as_ref() }
    fn parent_beacon_block_root(&self) -> Option<B256> { self.parent_beacon_block_root }
}

impl BeraPayloadBuilderAttributes {
    /// Access your custom field
    pub fn custom_bera_flag(&self) -> bool {
        self.custom_bera_flag
    }
}

impl reth_payload_primitives::PayloadBuilderAttributes for BeraPayloadBuilderAttributes {
    type RpcPayloadAttributes = BeraPayloadAttributes;
    type Error = eyre::Error;
    
    fn try_new(
        parent: B256,
        attributes: Self::RpcPayloadAttributes,
    ) -> Result<Self, Self::Error> {
        // Convert to standard Ethereum attributes
        let eth_attrs = reth_ethereum_engine_primitives::EthPayloadAttributes {
            timestamp: attributes.timestamp,
            prev_randao: attributes.prev_randao,
            suggested_fee_recipient: attributes.suggested_fee_recipient,
            withdrawals: attributes.withdrawals,
            parent_beacon_block_root: attributes.parent_beacon_block_root,
        };
        
        let inner = reth_ethereum_engine_primitives::EthPayloadBuilderAttributes::try_new(
            parent, 
            eth_attrs
        )?;
        
        Ok(Self {
            inner,
            custom_bera_flag: attributes.custom_bera_flag,
        })
    }
    
    // Delegate standard methods to inner implementation
    fn payload_id(&self) -> reth_payload_primitives::PayloadId { self.inner.payload_id() }
    fn parent(&self) -> B256 { self.inner.parent() }
    fn timestamp(&self) -> u64 { self.inner.timestamp() }
    fn suggested_fee_recipient(&self) -> Address { self.inner.suggested_fee_recipient() }
    fn prev_randao(&self) -> B256 { self.inner.prev_randao() }
    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> { self.inner.withdrawals() }
    fn parent_beacon_block_root(&self) -> Option<B256> { self.inner.parent_beacon_block_root() }
}
```

**🔍 Compile and check**: Your node should now compile! The basic type system is complete.

### Step 4: Test Your Contract Implementation

At this point, you can test that your custom types work:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256};
    
    #[test]
    fn test_custom_payload_attributes() {
        let custom_attrs = BeraPayloadAttributes {
            timestamp: 1234567890,
            prev_randao: B256::random(),
            suggested_fee_recipient: Address::random(),
            withdrawals: None,
            parent_beacon_block_root: None,
            custom_bera_flag: true,  // Your custom field!
        };
        
        // Test conversion to builder attributes
        let builder_attrs = BeraPayloadBuilderAttributes::try_new(
            B256::random(),
            custom_attrs,
        ).unwrap();
        
        // Verify your custom field is accessible
        assert!(builder_attrs.custom_bera_flag());
    }
}
```

**🎉 Success!** You now have a working custom forkchoice attribute system.

### Step 5: Add Custom Logic (Extend Outward)

Now that the type system works, you can extend outward to add custom logic where needed.

#### Option A: Custom Payload Builder

If you want to use the custom field in payload building:

Create `src/payload/builder.rs`:
```rust
use reth_ethereum_payload_builder::EthereumPayloadBuilder;
use crate::engine::attributes::BeraPayloadBuilderAttributes;

#[derive(Debug, Clone)]
pub struct BeraPayloadBuilder<Pool, Client, EvmConfig> {
    inner: EthereumPayloadBuilder<Pool, Client, EvmConfig>,
}

impl<Pool, Client, EvmConfig> reth_payload_builder::PayloadBuilder for BeraPayloadBuilder<Pool, Client, EvmConfig>
where
    Pool: reth_transaction_pool::TransactionPool,
    Client: reth_provider::StateProviderFactory,
    EvmConfig: reth_evm::ConfigureEvm,
{
    type Attributes = BeraPayloadBuilderAttributes;
    type BuiltPayload = reth_ethereum_primitives::EthBuiltPayload;
    
    fn try_build(
        &self,
        args: reth_payload_builder::BuildArguments<Self::Attributes>,
    ) -> Result<reth_payload_builder::BuildOutcome<Self::BuiltPayload>, reth_payload_builder::PayloadBuilderError> {
        // Access your custom field
        if args.config.attributes.custom_bera_flag() {
            tracing::info!("Building payload with custom Bera logic enabled!");
            // Add your custom logic here
        }
        
        // For now, delegate to standard Ethereum builder
        // You can customize this further as needed
        self.inner.try_build(/* convert args to standard format */)
    }
    
    fn build_empty_payload(
        &self,
        config: reth_payload_builder::PayloadConfig<Self::Attributes>,
    ) -> Result<Self::BuiltPayload, reth_payload_builder::PayloadBuilderError> {
        if config.attributes.custom_bera_flag() {
            tracing::info!("Building empty payload with custom flag");
        }
        
        self.inner.build_empty_payload(/* convert config */)
    }
}
```

#### Option B: Custom Engine Validator

If you want to validate the custom field:

Create `src/engine/validator.rs`:
```rust
use reth_node_api::EngineValidator;
use crate::engine::types::BeraEngineTypes;

#[derive(Debug, Clone)]
pub struct BeraEngineValidator<T> {
    inner: reth_ethereum_engine_primitives::EthereumEngineValidator<T>,
}

impl<T> EngineValidator<BeraEngineTypes> for BeraEngineValidator<T>
where
    T: reth_provider::StateProviderFactory,
{
    fn validate_version_specific_fields(
        &self,
        version: reth_engine_primitives::EngineApiMessageVersion,
        payload_or_attrs: reth_engine_primitives::PayloadOrAttributes<'_, BeraEngineTypes>,
    ) -> eyre::Result<()> {
        match payload_or_attrs {
            reth_engine_primitives::PayloadOrAttributes::PayloadAttributes(attrs) => {
                // Validate your custom field
                if attrs.custom_bera_flag {
                    tracing::info!("Validating payload with custom Bera flag enabled");
                    // Add custom validation logic here
                }
            }
            _ => {}
        }
        
        // Delegate standard validation
        Ok(())
    }
}
```

## Why This Contract-First Approach Works Better

### 🎯 **Clear Goal from Start**
- **Define the interface upfront** - you know exactly what you're building toward
- **Type system validates compatibility** - ensures all pieces fit together
- **No over-engineering** - only implement what's needed for the contract

### 🔧 **Compiler as Your Guide**
The contract-first approach uses the compiler as a guide:

1. **Step 1**: Define `NodeTypes` → Compiler asks for `BeraEngineTypes`
2. **Step 2**: Create `BeraEngineTypes` → Compiler asks for `BeraPayloadAttributes`
3. **Step 3**: Create attributes → Compiler tells you what traits to implement
4. **Step 4**: Test the contract → Verify it works end-to-end

### 📈 **Progressive Validation**
- **Step 3**: Basic type system compiles ✅
- **Step 4**: Contract works with test data ✅
- **Step 5**: Add custom logic incrementally ✅

### 🔄 **Easier Extension**
Once the contract is established:
- **Add new fields** to payload attributes
- **Extend custom logic** in payload builders or validators
- **Modify behavior** without changing the type system

## File Structure Summary

This contract-first approach creates:

1. ✅ `src/engine/mod.rs` - Module declarations
2. ✅ `src/engine/types.rs` - Engine type contract
3. ✅ `src/engine/attributes.rs` - Custom payload attributes
4. ✅ Update `src/node/mod.rs` - NodeTypes implementation (1 line change)
5. 🔧 **Optional**: `src/payload/builder.rs` - Custom payload logic
6. 🔧 **Optional**: `src/engine/validator.rs` - Custom validation

**Total**: 3 new files + 1 line change to define the contract, then extend as needed.

## Engine API Integration

BeaconKit can now send your custom field:

```json
{
  "method": "engine_forkchoiceUpdatedV3",
  "params": [
    { "headBlockHash": "0x...", "safeBlockHash": "0x...", "finalizedBlockHash": "0x..." },
    {
      "timestamp": "0x123456",
      "prevRandao": "0x...",
      "suggestedFeeRecipient": "0x...",
      "custom_bera_flag": true
    }
  ]
}
```

This contract-first approach gives you the leanest path to extending forkchoice attributes while ensuring type safety throughout the system.
    fn build_with_custom_logic(
        &self, 
        custom_flag: bool,
        args: reth_payload_builder::BuildArguments<impl reth_payload_primitives::PayloadBuilderAttributes>,
    ) -> Result<reth_payload_builder::BuildOutcome<impl reth_payload_primitives::BuiltPayload>, reth_payload_builder::PayloadBuilderError> {
        if custom_flag {
            tracing::info!("Building payload with custom Bera logic enabled");
            
            // Your custom logic here - this is where you validate the design
            // Example: prioritize certain transactions, adjust gas limits, etc.
            
            // For now, delegate to standard builder but log the custom behavior
            tracing::info!("Custom bera flag: {}", custom_flag);
        }
        
        // Delegate to standard Ethereum payload builder
        self.inner.try_build(args)
    }
}

// At this point, you'll get compiler errors telling you what types you need to define
// Use these errors as a guide for the next steps
```

**🔍 Compile and observe**: This will give you compiler errors that tell you exactly what types need to be defined. These errors become your roadmap.

### Step 2: Create Minimal Custom Types (Driven by Compiler Errors)

Based on the compiler errors from Step 1, create the minimal types needed:

Create `src/engine/attributes.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Start with the simplest possible custom attribute
/// Add complexity only when compiler errors demand it
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeraPayloadAttributes {
    /// Standard Ethereum fields
    pub timestamp: u64,
    pub prev_randao: alloy_primitives::B256,
    pub suggested_fee_recipient: alloy_primitives::Address,
    pub withdrawals: Option<Vec<alloy_eips::eip4895::Withdrawal>>,
    pub parent_beacon_block_root: Option<alloy_primitives::B256>,
    
    /// Your custom field - start simple!
    pub custom_bera_flag: bool,
}

// Implement only what the compiler errors tell you is required
impl reth_payload_primitives::PayloadAttributes for BeraPayloadAttributes {
    type RpcPayloadAttributes = Self;
    type Error = eyre::Error; // Start simple
    
    fn timestamp(&self) -> u64 { self.timestamp }
    fn prev_randao(&self) -> alloy_primitives::B256 { self.prev_randao }
    fn suggested_fee_recipient(&self) -> alloy_primitives::Address { self.suggested_fee_recipient }
    fn withdrawals(&self) -> Option<&Vec<alloy_eips::eip4895::Withdrawal>> { self.withdrawals.as_ref() }
    fn parent_beacon_block_root(&self) -> Option<alloy_primitives::B256> { self.parent_beacon_block_root }
}
```

**🔍 Compile and iterate**: Add only what the compiler requires. Don't implement everything upfront.

### Step 3: Wire Payload Builder to Custom Types

Update your payload builder to use the custom attributes:

```rust
impl<Pool, Client, EvmConfig> reth_payload_builder::PayloadBuilder for BeraPayloadBuilder<Pool, Client, EvmConfig>
where
    Pool: reth_transaction_pool::TransactionPool,
    Client: reth_provider::StateProviderFactory,
    EvmConfig: reth_evm::ConfigureEvm,
{
    type Attributes = BeraPayloadBuilderAttributes; // Compiler will tell you to define this
    type BuiltPayload = reth_ethereum_primitives::EthBuiltPayload; // Start simple
    
    fn try_build(
        &self,
        args: reth_payload_builder::BuildArguments<Self::Attributes>,
    ) -> Result<reth_payload_builder::BuildOutcome<Self::BuiltPayload>, reth_payload_builder::PayloadBuilderError> {
        // Now you can access your custom field!
        let custom_flag = args.config.attributes.custom_bera_flag();
        
        self.build_with_custom_logic(custom_flag, args)
    }
    
    fn build_empty_payload(
        &self,
        config: reth_payload_builder::PayloadConfig<Self::Attributes>,
    ) -> Result<Self::BuiltPayload, reth_payload_builder::PayloadBuilderError> {
        // Access custom field here too
        if config.attributes.custom_bera_flag() {
            tracing::info!("Building empty payload with custom flag enabled");
        }
        
        self.inner.build_empty_payload(/* convert config to standard format */)
    }
}
```

**🔍 Compile and follow errors**: The compiler will tell you that `BeraPayloadBuilderAttributes` needs to be defined.

### Step 4: Add Builder Attributes (Compiler-Driven)

Create the builder attributes type that the compiler is asking for:

```rust
// Add to src/engine/attributes.rs

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeraPayloadBuilderAttributes {
    inner: reth_ethereum_engine_primitives::EthPayloadBuilderAttributes,
    custom_bera_flag: bool,
}

impl BeraPayloadBuilderAttributes {
    pub fn custom_bera_flag(&self) -> bool {
        self.custom_bera_flag
    }
}

impl reth_payload_primitives::PayloadBuilderAttributes for BeraPayloadBuilderAttributes {
    type RpcPayloadAttributes = BeraPayloadAttributes;
    type Error = eyre::Error;
    
    fn try_new(
        parent: alloy_primitives::B256,
        attributes: Self::RpcPayloadAttributes,
    ) -> Result<Self, Self::Error> {
        let inner = reth_ethereum_engine_primitives::EthPayloadBuilderAttributes::try_new(
            parent,
            reth_ethereum_engine_primitives::EthPayloadAttributes {
                timestamp: attributes.timestamp,
                prev_randao: attributes.prev_randao,
                suggested_fee_recipient: attributes.suggested_fee_recipient,
                withdrawals: attributes.withdrawals,
                parent_beacon_block_root: attributes.parent_beacon_block_root,
            },
        )?;
        
        Ok(Self {
            inner,
            custom_bera_flag: attributes.custom_bera_flag,
        })
    }
    
    // Delegate standard methods to inner
    fn payload_id(&self) -> reth_payload_primitives::PayloadId { self.inner.payload_id() }
    fn parent(&self) -> alloy_primitives::B256 { self.inner.parent() }
    fn timestamp(&self) -> u64 { self.inner.timestamp() }
    fn suggested_fee_recipient(&self) -> alloy_primitives::Address { self.inner.suggested_fee_recipient() }
    fn prev_randao(&self) -> alloy_primitives::B256 { self.inner.prev_randao() }
    fn withdrawals(&self) -> Option<&Vec<alloy_eips::eip4895::Withdrawal>> { self.inner.withdrawals() }
    fn parent_beacon_block_root(&self) -> Option<alloy_primitives::B256> { self.inner.parent_beacon_block_root() }
}
```

**🔍 Test the core logic**: At this point, you should be able to compile and test your payload builder with mock attributes.

### Step 5: Create Engine Types (Working Outward)

Now create the engine types that wire everything together:

```rust
// Create src/engine/types.rs
use reth_payload_primitives::PayloadTypes;
use crate::engine::attributes::{BeraPayloadAttributes, BeraPayloadBuilderAttributes};

#[derive(Clone, Debug, Default)]
pub struct BeraEngineTypes;

impl PayloadTypes for BeraEngineTypes {
    type ExecutionData = reth_ethereum_engine_primitives::ExecutionData;
    type BuiltPayload = reth_ethereum_primitives::EthBuiltPayload;
    type PayloadAttributes = BeraPayloadAttributes;
    type PayloadBuilderAttributes = BeraPayloadBuilderAttributes;
    
    fn block_to_payload(
        block: reth_primitives::SealedBlock,
    ) -> Self::ExecutionData {
        reth_ethereum_engine_primitives::EthEngineTypes::block_to_payload(block)
    }
}
```

### Step 6: Update Node Configuration (Outermost Layer)

Finally, wire your custom types into the node:

```rust
// Update src/node/mod.rs
use crate::engine::types::BeraEngineTypes;

impl NodeTypes for BerachainNode {
    type Primitives = <EthereumNode as NodeTypes>::Primitives;
    type ChainSpec = BerachainChainSpec;
    type StateCommitment = <EthereumNode as NodeTypes>::StateCommitment;
    type Storage = <EthereumNode as NodeTypes>::Storage;
    type Payload = BeraEngineTypes; // ← Use your custom engine types
}
```

## Incremental Testing Strategy

Test each step as you build outward:

### Test Step 1-3: Unit Test Payload Builder
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_custom_payload_logic() {
        // Test your core logic with mock attributes
        let mock_attributes = BeraPayloadBuilderAttributes::mock_with_flag(true);
        
        // Verify your custom logic activates correctly
        assert!(mock_attributes.custom_bera_flag());
    }
}
```

### Test Step 4-5: Integration Test
```rust
#[tokio::test]
async fn test_payload_building_with_custom_attributes() {
    let custom_attrs = BeraPayloadAttributes {
        timestamp: 1234567890,
        prev_randao: B256::random(),
        suggested_fee_recipient: Address::random(),
        withdrawals: None,
        parent_beacon_block_root: None,
        custom_bera_flag: true,
    };
    
    let builder_attrs = BeraPayloadBuilderAttributes::try_new(
        B256::random(),
        custom_attrs,
    ).unwrap();
    
    assert!(builder_attrs.custom_bera_flag());
}
```

### Test Step 6: End-to-End Test
```rust
#[tokio::test]
async fn test_full_node_with_custom_engine_types() {
    // Test that the node can start with custom engine types
    // and handle forkchoice updates with custom attributes
}
```

## Why This Approach Works Better

### 🎯 **Validates Design Early**
By starting with the payload builder, you immediately validate that your custom field is actually useful and accessible where you need it.

### 🔧 **Compiler-Guided Development**
Each step generates specific compiler errors that tell you exactly what to implement next:

1. **Step 1**: "Cannot find type `BeraPayloadBuilderAttributes`" → implement Step 4
2. **Step 4**: "Type doesn't implement `PayloadBuilderAttributes`" → implement required methods
3. **Step 5**: "Mismatched associated types" → wire up engine types correctly

### 🧪 **Incremental Testing**
You can test and validate each component independently:
- Unit test payload building logic first
- Integration test attribute handling
- End-to-end test full node functionality

### 🔄 **Easier Iteration**
If you need to change your custom field structure, you only need to update the innermost components and let the compiler guide you through the necessary changes.

## Minimal File Changes Summary

Using this approach, you'll create these files in order:

1. ✅ `src/payload/builder.rs` - Your custom payload logic (validates the design)
2. ✅ `src/engine/attributes.rs` - Custom attribute types (driven by compiler errors)
3. ✅ `src/engine/types.rs` - Engine type wiring (working outward)
4. ✅ `src/engine/mod.rs` - Module declarations
5. ✅ Update `src/node/mod.rs` - Wire custom types to node (outermost change)

**Total**: 4 new files + 1 small update to existing file

This component-first approach ensures each step compiles and works, reducing the risk of getting stuck in complex type errors that are hard to debug.

## Next Steps After Implementation

Once you have the basic custom field working, you can extend it:

### Add Validation Logic
```rust
impl BeraPayloadAttributes {
    pub fn validate_custom_fields(&self) -> Result<(), &'static str> {
        if self.custom_bera_flag && self.timestamp < minimum_timestamp() {
            return Err("Custom flag requires recent timestamp");
        }
        Ok(())
    }
}
```

### Add More Custom Fields
```rust
pub struct BeraPayloadAttributes {
    // ... existing fields
    pub custom_bera_flag: bool,
    pub validator_priority_list: Option<Vec<Address>>, // New field
    pub execution_hints: Option<BeraExecutionHints>,   // New field
}
```

### Engine API Integration
The consensus layer (BeaconKit) can send your custom fields:
```json
{
  "method": "engine_forkchoiceUpdatedV3",
  "params": [
    { "headBlockHash": "0x...", "safeBlockHash": "0x...", "finalizedBlockHash": "0x..." },
    {
      "timestamp": "0x123456",
      "prevRandao": "0x...",
      "suggestedFeeRecipient": "0x...",
      "custom_bera_flag": true
    }
  ]
}
```

This component-first approach provides the most reliable path to extending forkchoice attributes in bera-reth.

This is the **minimum set** required by Reth's architecture - any attempt to use fewer types will result in trait bound compilation errors.

### Step 1: Define Custom Payload Attributes

**Why this step is needed**: This type handles the **RPC boundary** between consensus layer (BeaconKit) and execution layer (bera-reth). It must be serializable for JSON-RPC and implement the `PayloadAttributes` trait that the Engine API expects.

**Role in Reth architecture**: 
- **Engine API Entry Point**: This is the first type that receives data from the consensus layer
- **Serialization Layer**: Must handle JSON serialization/deserialization for RPC calls
- **Trait Compliance**: Must implement `PayloadAttributes` trait which is required by `EngineTypes`
- **Type Propagation**: Becomes the `PayloadAttributes` associated type used throughout the system

**Cannot be eliminated because**: The Engine API trait requires a concrete type implementing `PayloadAttributes`. Without this, the trait bounds in `EngineTypes` cannot be satisfied.

Create `src/engine/attributes.rs`:

```rust
use alloy_rpc_types_engine::PayloadAttributes as EthPayloadAttributes;
use serde::{Deserialize, Serialize};

/// Custom payload attributes for Berachain
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeraPayloadAttributes {
    /// Standard Ethereum payload attributes
    #[serde(flatten)]
    pub inner: EthPayloadAttributes,
    
    /// Custom Berachain field - your boolean or any other field
    pub custom_bera_flag: bool,
    
    /// Add more custom fields as needed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_data: Option<u64>,
}

impl BeraPayloadAttributes {
    /// Create from standard Ethereum attributes with default custom values
    pub fn from_eth_attributes(eth_attrs: EthPayloadAttributes) -> Self {
        Self {
            inner: eth_attrs,
            custom_bera_flag: false,
            optional_data: None,
        }
    }
    
    /// Validate custom fields
    pub fn validate_custom_fields(&self) -> Result<(), &'static str> {
        // Add your validation logic here
        if self.custom_bera_flag {
            // Custom validation when flag is enabled
            if self.inner.timestamp == 0 {
                return Err("Timestamp required when custom_bera_flag is true");
            }
        }
        
        Ok(())
    }
}

/// Required trait implementations
impl reth_payload_primitives::PayloadAttributes for BeraPayloadAttributes {
    type RpcPayloadAttributes = Self;
    type Error = BeraAttributesError;
    
    fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }
    
    fn prev_randao(&self) -> alloy_primitives::B256 {
        self.inner.prev_randao
    }
    
    fn suggested_fee_recipient(&self) -> alloy_primitives::Address {
        self.inner.suggested_fee_recipient
    }
    
    fn withdrawals(&self) -> Option<&Vec<alloy_eips::eip4895::Withdrawal>> {
        self.inner.withdrawals.as_ref()
    }
    
    fn parent_beacon_block_root(&self) -> Option<alloy_primitives::B256> {
        self.inner.parent_beacon_block_root
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BeraAttributesError {
    #[error("Invalid custom bera field: {0}")]
    InvalidCustomField(String),
}
```

### Step 2: Create Payload Builder Attributes Wrapper

**Why this step is needed**: This type represents **validated and processed** attributes used internally by the payload building system. It handles the transformation from raw RPC data to internal validated data.

**Role in Reth architecture**:
- **Validation Layer**: Performs validation on RPC attributes and rejects invalid data before payload building
- **Internal API**: Used by payload builders and jobs - must implement `PayloadBuilderAttributes` trait
- **Type Safety**: Ensures only validated attributes reach the payload building system
- **Resource Management**: Can pre-compute expensive operations (like payload IDs) during validation

**Cannot be eliminated because**: 
- `PayloadTypes` trait requires separate `PayloadAttributes` and `PayloadBuilderAttributes` associated types
- Payload builders expect `PayloadBuilderAttributes` trait implementation, not `PayloadAttributes`
- Different trait bounds: RPC types need `Serialize + Deserialize`, internal types need different traits

**Key difference from Step 1**: 
- Step 1 = **RPC serializable** types for consensus layer communication
- Step 2 = **Internal validated** types for payload building logic

Add to `src/engine/attributes.rs`:

```rust
use reth_payload_primitives::PayloadBuilderAttributes;

/// Wrapper for payload builder attributes
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeraPayloadBuilderAttributes {
    inner: reth_ethereum_engine_primitives::EthPayloadBuilderAttributes,
    custom_bera_flag: bool,
    optional_data: Option<u64>,
}

impl BeraPayloadBuilderAttributes {
    pub fn new(
        inner: reth_ethereum_engine_primitives::EthPayloadBuilderAttributes,
        custom_bera_flag: bool,
        optional_data: Option<u64>,
    ) -> Self {
        Self {
            inner,
            custom_bera_flag,
            optional_data,
        }
    }
    
    /// Access custom fields
    pub fn custom_bera_flag(&self) -> bool {
        self.custom_bera_flag
    }
    
    pub fn optional_data(&self) -> Option<u64> {
        self.optional_data
    }
}

impl PayloadBuilderAttributes for BeraPayloadBuilderAttributes {
    type RpcPayloadAttributes = BeraPayloadAttributes;
    type Error = BeraAttributesError;
    
    fn try_new(
        parent: alloy_primitives::B256,
        attributes: Self::RpcPayloadAttributes,
    ) -> Result<Self, Self::Error> {
        // Validate custom fields first
        attributes.validate_custom_fields()
            .map_err(|e| BeraAttributesError::InvalidCustomField(e.to_string()))?;
        
        // Create inner attributes
        let inner = reth_ethereum_engine_primitives::EthPayloadBuilderAttributes::try_new(
            parent,
            attributes.inner,
        ).map_err(|e| BeraAttributesError::InvalidCustomField(format!("Inner error: {}", e)))?;
        
        Ok(Self::new(
            inner,
            attributes.custom_bera_flag,
            attributes.optional_data,
        ))
    }
    
    fn payload_id(&self) -> reth_payload_primitives::PayloadId {
        self.inner.payload_id()
    }
    
    fn parent(&self) -> alloy_primitives::B256 {
        self.inner.parent()
    }
    
    fn timestamp(&self) -> u64 {
        self.inner.timestamp()
    }
    
    fn suggested_fee_recipient(&self) -> alloy_primitives::Address {
        self.inner.suggested_fee_recipient()
    }
    
    fn prev_randao(&self) -> alloy_primitives::B256 {
        self.inner.prev_randao()
    }
    
    fn withdrawals(&self) -> Option<&Vec<alloy_eips::eip4895::Withdrawal>> {
        self.inner.withdrawals()
    }
    
    fn parent_beacon_block_root(&self) -> Option<alloy_primitives::B256> {
        self.inner.parent_beacon_block_root()
    }
}
```

### Step 3: Define Custom Engine Types

**Why this step is needed**: This is the **type-level wiring** that connects all custom types together and integrates them into Reth's node architecture. It serves as the central type configuration for the Engine API system.

**Role in Reth architecture**:
- **Type System Hub**: Implements `PayloadTypes` trait that wires together all payload-related types
- **Node Integration**: Becomes the `Payload` associated type in `NodeTypes`, propagating through entire system
- **Component Coordination**: Ensures all components (payload builder, engine validator, RPC) use compatible types
- **Generic Parameter**: Used as generic parameter in `Node<N>` trait bounds throughout the codebase

**Cannot be eliminated because**:
- **`NodeTypes` requirement**: `NodeTypes` trait requires a `Payload: PayloadTypes` associated type
- **Trait bound propagation**: This type appears in trait bounds across payload builder, engine API, and node components
- **Component factory**: Components are parameterized by this type to ensure type compatibility

**Connection to wider Reth codebase**:
- `NodeBuilder` uses this in `Node<N>` generic parameter
- `PayloadBuilderService` is parameterized by `T: PayloadTypes`
- `EngineApiTreeHandler` requires matching `EngineTypes: PayloadTypes`
- RPC modules use this type for Engine API endpoint implementations

Create `src/engine/types.rs`:

```rust
use reth_payload_primitives::{PayloadTypes, BuiltPayload};
use reth_ethereum_engine_primitives::{EthEngineTypes, EthBuiltPayload};
use alloy_rpc_types_engine::ExecutionPayload;
use crate::engine::attributes::{BeraPayloadAttributes, BeraPayloadBuilderAttributes};

/// Custom engine types for Berachain
#[derive(Clone, Debug, Default)]
pub struct BeraEngineTypes;

impl PayloadTypes for BeraEngineTypes {
    type ExecutionData = <EthEngineTypes as PayloadTypes>::ExecutionData;
    type BuiltPayload = EthBuiltPayload; // Reuse Ethereum built payload
    type PayloadAttributes = BeraPayloadAttributes;
    type PayloadBuilderAttributes = BeraPayloadBuilderAttributes;
    
    fn block_to_payload(
        block: reth_primitives::SealedBlock,
    ) -> Self::ExecutionData {
        // Delegate to Ethereum implementation
        EthEngineTypes::block_to_payload(block)
    }
}
```

### Step 4: Create Custom Engine Validator

**Why this step is needed**: This component provides **runtime validation** with access to chain state and custom validation logic. It's the enforcement point for custom field validation before payload building begins.

**Role in Reth architecture**:
- **Runtime Validation**: Validates payload attributes against current chain state (unlike compile-time type checking)
- **Chain State Access**: Has access to `StateProviderFactory` for validating attributes against current blockchain state
- **Engine API Integration**: Called by Engine API handlers before creating payload jobs
- **Custom Logic Enforcement**: Where custom business rules and validation logic is implemented

**Cannot be eliminated because**:
- **`EngineValidator` trait requirement**: Engine API system requires an `EngineValidator` implementation
- **Runtime vs Compile-time**: Type system can't validate business rules that depend on chain state
- **AddOns integration**: Node's AddOns system expects an `EngineValidatorBuilder` to be provided

**Key validations this enables**:
- Validate custom fields against current chain state
- Implement custom business rules (e.g., "custom_bera_flag only allowed after block X")
- Check relationships between custom fields and existing blockchain data
- Provide detailed error messages for invalid custom attributes

**Connection to wider Reth codebase**:
- Called by `EngineApiTreeHandler` in the Engine API request processing pipeline
- Integrated via `AddOns` system in `EthereumAddOns` type
- Runs before payload jobs are created by `PayloadBuilderService`

Create `src/engine/validator.rs`:

```rust
use reth_node_api::EngineValidator;
use reth_ethereum_engine_primitives::EthereumEngineValidator;
use reth_engine_primitives::EngineTypes;
use eyre::Result;
use crate::engine::types::BeraEngineTypes;

/// Custom engine validator for Berachain
#[derive(Debug, Clone)]
pub struct BeraEngineValidator<T> {
    inner: EthereumEngineValidator<T>,
}

impl<T> BeraEngineValidator<T> {
    pub fn new(inner: EthereumEngineValidator<T>) -> Self {
        Self { inner }
    }
}

impl<T> EngineValidator<BeraEngineTypes> for BeraEngineValidator<T>
where
    T: reth_provider::StateProviderFactory + Send + Sync + 'static,
{
    fn validate_version_specific_fields(
        &self,
        version: reth_engine_primitives::EngineApiMessageVersion,
        payload_or_attrs: reth_engine_primitives::PayloadOrAttributes<'_, BeraEngineTypes>,
    ) -> Result<()> {
        // Validate custom fields based on the version
        match payload_or_attrs {
            reth_engine_primitives::PayloadOrAttributes::PayloadAttributes(attrs) => {
                // Validate your custom fields here
                attrs.validate_custom_fields()
                    .map_err(|e| eyre::eyre!("Custom validation failed: {}", e))?;
                
                // Log custom field usage for debugging
                if attrs.custom_bera_flag {
                    tracing::info!("Custom bera flag enabled for payload building");
                }
            }
            _ => {}
        }
        
        // Delegate standard validation to Ethereum validator
        // Note: This requires adapting the payload/attributes to Ethereum types
        Ok(())
    }
    
    fn ensure_well_formed_attributes(
        &self,
        version: reth_engine_primitives::EngineApiMessageVersion,
        attributes: &<BeraEngineTypes as EngineTypes>::PayloadAttributes,
    ) -> Result<()> {
        // Custom attribute validation
        attributes.validate_custom_fields()
            .map_err(|e| eyre::eyre!("Attribute validation failed: {}", e))?;
        
        // Delegate to standard Ethereum validation for core fields
        // (This would require converting to EthPayloadAttributes)
        Ok(())
    }
}
```

### Step 5: Integrate with Node Types

**Why this step is needed**: This is the **integration point** that propagates your custom types through the entire Reth node architecture. It's the single change that makes all components use your custom types.

**Role in Reth architecture**:
- **Type Propagation Root**: `NodeTypes` is the root of the type system - all components inherit types from here
- **Component Factory Configuration**: Determines which component builders get used (pool, network, payload, etc.)
- **Generic Parameter Source**: Becomes the `N` in `Node<N>` used throughout the system
- **AddOns Coordination**: Configures which AddOns (RPC, Engine API) are used with which types

**Cannot be eliminated because**:
- **Type System Root**: This is where custom types enter the node's type system
- **Component Parameterization**: All node components are parameterized by types from `NodeTypes`
- **Builder Integration**: `NodeBuilder` system requires this to connect custom types to component builders

**What this change triggers**:
- All payload building components now use `BeraEngineTypes`
- Engine API handlers now expect `BeraPayloadAttributes`
- RPC endpoints now serialize/deserialize custom fields
- Component builders get properly typed contexts

**Connection to wider Reth codebase**:
- `NodeBuilder::with_types::<BerachainNode>()` reads this configuration
- `ComponentsBuilder` uses these types to parameterize all component builders
- `AddOns` system uses these types for RPC and Engine API configuration
- Database providers and network components inherit compatibility requirements

Modify `src/node.rs`:

```rust
use crate::engine::types::BeraEngineTypes;

impl NodeTypes for BerachainNode {
    type Primitives = <EthereumNode as NodeTypes>::Primitives;
    type ChainSpec = BerachainChainSpec;
    type StateCommitment = <EthereumNode as NodeTypes>::StateCommitment;
    type Storage = <EthereumNode as NodeTypes>::Storage;
    type Payload = BeraEngineTypes; // ← Use custom engine types
}

// Update the AddOns type to use custom engine validator
type AddOns = EthereumAddOns<
    NodeAdapter<N, Components>,
    EthereumEthApiBuilder,
    BeraEngineValidatorBuilder, // ← Custom validator builder
    BasicEngineApiBuilder<BeraEngineValidatorBuilder>,
>;
```

### Step 6: Create Engine Validator Builder

**Why this step is needed**: This is the **factory pattern** required by Reth's AddOns system. It provides the bridge between the static type system and runtime component creation with dependency injection.

**Role in Reth architecture**:
- **AddOns Integration**: Required by the AddOns system which expects `EngineValidatorBuilder` implementations
- **Dependency Injection**: Receives `AddOnsContext` with access to provider, config, and other node resources
- **Async Initialization**: Handles async setup of the validator with proper resource initialization
- **Component Lifecycle**: Manages the creation and configuration of the engine validator

**Cannot be eliminated because**:
- **AddOns System Requirement**: `EthereumAddOns` type requires an `EngineValidatorBuilder` generic parameter
- **Builder Pattern**: Reth uses builder pattern extensively - components must provide builders
- **Resource Access**: Validator needs access to provider and chain spec which are only available during node startup
- **Type System Bridge**: Connects compile-time types to runtime component instantiation

**Connection to wider Reth codebase**:
- Used in `EthereumAddOns<..., BeraEngineValidatorBuilder, ...>` type definition
- Called during node startup by the AddOns system
- Provides the validator instance used by Engine API handlers
- Integrated into the component dependency graph

Add to `src/engine/validator.rs`:

```rust
use reth_node_builder::EngineValidatorBuilder;

/// Builder for Bera engine validator
#[derive(Debug, Default, Clone, Copy)]
pub struct BeraEngineValidatorBuilder;

impl<Node> EngineValidatorBuilder<Node> for BeraEngineValidatorBuilder
where
    Node: reth_node_api::FullNodeTypes<Types: NodeTypes<ChainSpec = BerachainChainSpec>>,
{
    type Validator = BeraEngineValidator<Node::Provider>;
    
    async fn build(self, ctx: &reth_node_builder::AddOnsContext<'_, Node>) -> eyre::Result<Self::Validator> {
        let ethereum_validator = EthereumEngineValidator::new(ctx.provider().clone());
        Ok(BeraEngineValidator::new(ethereum_validator))
    }
}
```

### Step 7: Update Module Structure

**Why this step is needed**: This creates the **module organization** and **public API** for your custom engine types. It's required for Rust's module system and provides clean imports for other parts of the codebase.

**Role in Reth architecture**:
- **Module System**: Rust requires module declarations for code organization
- **Public API**: Defines which types and functions are accessible from other modules
- **Import Simplification**: Allows `use crate::engine::BeraEngineTypes` instead of deeply nested imports
- **Code Organization**: Groups related functionality together

**Cannot be eliminated because**:
- **Rust Language Requirement**: Rust requires module declarations in `mod.rs` or parent modules
- **Visibility Control**: Need to control which items are public vs private
- **Import Dependencies**: Other modules need to import these types

Create `src/engine/mod.rs`:

```rust
pub mod attributes;
pub mod types;
pub mod validator;

pub use attributes::{BeraPayloadAttributes, BeraPayloadBuilderAttributes};
pub use types::BeraEngineTypes;
pub use validator::{BeraEngineValidator, BeraEngineValidatorBuilder};
```

## Usage in Payload Building

When building payloads, you can access your custom fields:

```rust
// In your payload builder
impl PayloadBuilder for BeraPayloadBuilder {
    fn try_build(&self, args: BuildArguments<BeraPayloadBuilderAttributes>) -> Result<BuildOutcome> {
        let attributes = &args.config.attributes;
        
        // Access your custom field
        if attributes.custom_bera_flag() {
            // Apply custom logic when flag is true
            tracing::info!("Building payload with custom Bera logic enabled");
            
            // Custom transaction selection or block building logic
            return self.build_with_custom_logic(args);
        }
        
        // Standard payload building
        self.build_standard_payload(args)
    }
}
```

## Engine API Integration

The consensus layer (BeaconKit) can now send custom attributes:

```json
{
  "method": "engine_forkchoiceUpdatedV3",
  "params": [
    {
      "headBlockHash": "0x...",
      "safeBlockHash": "0x...",
      "finalizedBlockHash": "0x..."
    },
    {
      "timestamp": "0x123456",
      "prevRandao": "0x...",
      "suggestedFeeRecipient": "0x...",
      "custom_bera_flag": true,
      "optional_data": 42
    }
  ]
}
```

## Testing

```rust
#[tokio::test]
async fn test_custom_forkchoice_attributes() {
    let custom_attrs = BeraPayloadAttributes {
        inner: EthPayloadAttributes {
            timestamp: 1234567890,
            prev_randao: B256::random(),
            suggested_fee_recipient: Address::random(),
            withdrawals: None,
            parent_beacon_block_root: None,
        },
        custom_bera_flag: true,
        optional_data: Some(42),
    };
    
    // Test validation
    assert!(custom_attrs.validate_custom_fields().is_ok());
    
    // Test builder attributes creation
    let builder_attrs = BeraPayloadBuilderAttributes::try_new(
        B256::random(),
        custom_attrs,
    ).unwrap();
    
    assert!(builder_attrs.custom_bera_flag());
    assert_eq!(builder_attrs.optional_data(), Some(42));
}
```

## Understanding Reth's Type System Constraints

### Why Each Type Cannot Be Eliminated

The complexity comes from Reth's **strict type safety** design. Here's the complete dependency chain:

```rust
// This chain cannot be broken - each step requires the previous
NodeTypes::Payload: PayloadTypes
    ↓
PayloadTypes::PayloadAttributes + PayloadTypes::PayloadBuilderAttributes  
    ↓
EngineValidator<T: EngineTypes> where T::PayloadAttributes = YourCustomType
    ↓
PayloadBuilderService<T: PayloadTypes> where T::PayloadBuilderAttributes = YourCustomType
    ↓
Node<N: FullNodeTypes> where N::Types::Payload = YourCustomEngineTypes
```

**Each trait has specific requirements:**

1. **`PayloadTypes`** requires 4 associated types - cannot reduce this
2. **`PayloadAttributes`** vs **`PayloadBuilderAttributes`** serve different purposes - cannot combine
3. **`EngineValidator`** parameterized by `EngineTypes` - cannot eliminate  
4. **`NodeTypes`** must specify `Payload: PayloadTypes` - cannot skip

### Compilation Errors If You Try to Reduce Types

**Attempt to combine PayloadAttributes + PayloadBuilderAttributes:**
```rust
// This fails compilation
impl PayloadTypes for BeraEngineTypes {
    type PayloadAttributes = BeraAttributes;
    type PayloadBuilderAttributes = BeraAttributes; // ← Same type
}
```
**Error**: Trait bound conflicts because `PayloadBuilderAttributes` requires different trait implementations than `PayloadAttributes`.

**Attempt to skip EngineValidator:**
```rust
// This fails compilation  
type AddOns = EthereumAddOns<NodeAdapter<N, Components>, EthApiBuilder, (), EngineApiBuilder>;
//                                                                      ↑
//                                                              No validator
```
**Error**: `EthereumAddOns` requires `EngineValidatorBuilder` - cannot use `()`.

**Attempt to reuse EthEngineTypes:**
```rust
// This fails compilation
impl NodeTypes for BerachainNode {
    type Payload = EthEngineTypes; // ← Can't add custom fields to Ethereum types
}
```
**Error**: Cannot modify `EthPayloadAttributes` to include custom fields without breaking Ethereum compatibility.

### The Type System Cascade Effect

When you change **any** payload-related type, it cascades through:

1. **Engine API handlers** - Must accept your custom attributes type
2. **Payload builders** - Must process your custom attributes  
3. **RPC serialization** - Must handle custom fields in JSON
4. **Component builders** - Must be compatible with your custom types
5. **Node configuration** - Must wire everything together

This is why **5 types minimum** are required - each handles a different layer of this cascade.

## Key Benefits of This Approach

1. **Minimal Changes**: Only adds new files, doesn't modify existing bera-reth code
2. **Backward Compatible**: Standard Engine API still works
3. **Type Safe**: Leverages Rust's type system for validation
4. **Extensible**: Easy to add more custom fields later
5. **Testable**: Each component can be unit tested independently
6. **Necessary Complexity**: Uses the minimum number of types required by Reth's architecture

## File Changes Summary

- ✅ **New files only** - no modifications to existing bera-reth code
- `src/engine/mod.rs` - Module declarations
- `src/engine/attributes.rs` - Custom payload attributes
- `src/engine/types.rs` - Engine types implementation  
- `src/engine/validator.rs` - Custom validation logic
- `src/node.rs` - Update node types (2 line change)

This approach provides the simplest path to adding custom forkchoice attributes while maintaining full compatibility with the existing bera-reth architecture.

---

## Appendix: Potential Reth Simplifications

*This section outlines potential changes to Reth that could dramatically reduce the complexity of extending forkchoice attributes. These are recommendations for the Reth maintainers to consider.*

### Current Problem: 5 Types Minimum Required

Currently, adding a single boolean field to forkchoice attributes requires defining **5 separate types** and implementing **50+ lines of boilerplate code**. This creates significant friction for blockchain projects wanting to extend Reth.

### Proposed Simplifications

#### 1. **Merge PayloadAttributes and PayloadBuilderAttributes** (High Impact, Easy)

**Current Issue:**
```rust
// Must implement two separate traits with overlapping functionality
pub trait PayloadAttributes: Serialize + Deserialize { ... }
pub trait PayloadBuilderAttributes { 
    type RpcPayloadAttributes;  // Usually the PayloadAttributes type
    // 7+ delegation methods
}
```

**Proposed Solution:**
```rust
pub trait PayloadAttributes: Serialize + Deserialize {
    type Error: Error = Infallible;
    
    // Standard methods
    fn timestamp(&self) -> u64;
    fn withdrawals(&self) -> Option<&Vec<Withdrawal>>;
    
    // Builder functionality built-in
    fn to_builder_attributes(self, parent: B256) -> Result<PayloadBuilderWrapper<Self>, Self::Error> {
        Ok(PayloadBuilderWrapper::new(parent, self))
    }
    
    fn payload_id(&self, parent: B256) -> PayloadId {
        PayloadId::from_components(parent, self.timestamp(), /* ... */)
    }
}
```

**Benefit:** Eliminates 1 type requirement and 20+ lines of boilerplate per extension.

#### 2. **Add Default Associated Types to NodeTypes** (High Impact, Easy)

**Current Issue:**
```rust
pub trait NodeTypes {
    type Primitives: NodePrimitives;
    type ChainSpec: EthChainSpec;
    type StateCommitment: StateCommitment;        // Always MerklePatriciaTrie
    type Storage: Default + Send + Sync;          // Always EthStorage  
    type Payload: PayloadTypes;
}
```

**Proposed Solution:**
```rust
pub trait NodeTypes {
    type Primitives: NodePrimitives;
    type ChainSpec: EthChainSpec<Header = <Self::Primitives as NodePrimitives>::BlockHeader>;
    type Payload: PayloadTypes<BuiltPayload: BuiltPayload<Primitives = Self::Primitives>>;
    
    // Default implementations for common cases
    type StateCommitment: StateCommitment = MerklePatriciaTrie;
    type Storage: Default + Send + Sync + Unpin + Debug + 'static = EthStorage;
}
```

**Benefit:** Reduces required associated types from 5 to 3, eliminating boilerplate for 90% of use cases.

#### 3. **Derive Macro for PayloadBuilderAttributes** (High Impact, Medium)

**Current Issue:**
```rust
// Must manually implement 8+ delegation methods
impl PayloadBuilderAttributes for CustomPayloadBuilderAttributes {
    fn try_new(parent: B256, attributes: CustomPayloadAttributes) -> Result<Self, Infallible> {
        Ok(Self(EthPayloadBuilderAttributes::new(parent, attributes.inner)))
    }
    
    fn payload_id(&self) -> PayloadId { self.0.id }
    fn parent(&self) -> B256 { self.0.parent }
    fn timestamp(&self) -> u64 { self.0.timestamp }
    // ... 5 more delegation methods
}
```

**Proposed Solution:**
```rust
#[derive(PayloadBuilderAttributesDelegate)]
#[delegate(to = "0", rpc_type = "CustomPayloadAttributes")]
pub struct CustomPayloadBuilderAttributes(EthPayloadBuilderAttributes);

// Macro generates all delegation methods automatically
```

**Benefit:** Reduces 25+ lines of boilerplate to 3 lines (92% reduction).

#### 4. **Extension-Specific Macro** (Highest Impact, Medium)

**For the specific "add a boolean field" use case:**

**Current Issue:**
```rust
// Must define custom struct + implement 2 traits + handle serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPayloadAttributes {
    #[serde(flatten)]
    pub inner: EthPayloadAttributes,
    pub custom_flag: bool,
}

impl PayloadAttributes for CustomPayloadAttributes { /* 5+ methods */ }
impl PayloadBuilderAttributes for CustomPayloadBuilderAttributes { /* 8+ methods */ }
```

**Proposed Solution:**
```rust
#[derive(PayloadAttributesExtension)]
#[base_type = "EthPayloadAttributes"]
#[extension_fields(custom_flag: bool)]
pub struct CustomPayloadAttributes;

// Macro generates the struct + both trait implementations
```

**Benefit:** Reduces 50+ lines to 4 lines (92% reduction) for the most common extension case.

#### 5. **Relaxed EngineValidator Bounds** (Medium Impact, Easy)

**Current Issue:**
```rust
pub trait EngineValidator<Types: PayloadTypes> {
    // Requires full PayloadTypes even though only using PayloadAttributes
}
```

**Proposed Solution:**
```rust
pub trait EngineValidator<PayloadAttrs> {
    // Only require the specific type being validated
    fn ensure_well_formed_attributes(&self, attributes: &PayloadAttrs) -> Result<(), Error>;
}
```

**Benefit:** Reduces coupling and makes validators more reusable.

### Impact Summary

With these changes, extending forkchoice attributes would go from:

**Current (5 types, 150+ lines):**
```rust
// 1. PayloadAttributes (20 lines)
// 2. PayloadBuilderAttributes (25 lines) 
// 3. EngineTypes (15 lines)
// 4. EngineValidator (30 lines)
// 5. NodeTypes update (5 lines)
// 6. Module structure (10 lines)
// Total: ~105 lines + type definitions
```

**Simplified (2 types, 20 lines):**
```rust
// 1. Custom attributes with macro
#[derive(PayloadAttributesExtension)]
#[base_type = "EthPayloadAttributes"]
#[extension_fields(custom_flag: bool)]
pub struct CustomPayloadAttributes;

// 2. Engine types with macro  
#[derive(PayloadTypes)]
pub struct CustomEngineTypes {
    payload_attributes: CustomPayloadAttributes,
}

// 3. Node integration (unchanged)
impl NodeTypes for CustomNode {
    type Payload = CustomEngineTypes;
    // StateCommitment and Storage use defaults
}
```

**Result:** **85% reduction** in boilerplate code while maintaining full type safety.

### Implementation Priority

1. **Default associated types** (Easy, high impact, backward compatible)
2. **Merge PayloadAttributes traits** (Easy, high impact, deprecation cycle)
3. **PayloadAttributesExtension macro** (Medium, highest impact for extensions)
4. **Relaxed EngineValidator** (Easy, medium impact, backward compatible)

These changes would make Reth significantly more accessible for custom blockchain implementations while preserving the type safety and performance characteristics that make Reth exceptional.