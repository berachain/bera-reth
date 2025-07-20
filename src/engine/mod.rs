//! Berachain engine types and validation
//!
//! This module provides Berachain-specific implementations of engine types
//! required for the Engine API, while maintaining compatibility with Ethereum
//! through delegation to standard implementations where appropriate.
//!
//! Key components:
//! - [`BerachainEngineTypes`]: Main engine type configuration
//! - [`BerachainPayloadAttributes`]: Berachain-specific payload attributes
//! - [`builder::BerachainPayloadServiceBuilder`]: Service builder for payload integration
//! - [`builder::BerachainPayloadBuilder`]: Actual payload building implementation
//! - [`validator::BerachainEngineValidator`]: Engine validation logic

pub mod builder;
pub mod payload;
pub mod rpc;
pub mod validator;

use crate::{
    engine::payload::{
        BerachainBuiltPayload, BerachainPayloadAttributes, BerachainPayloadBuilderAttributes,
    },
    hardforks::BerachainHardforks,
    node::evm::error::BerachainExecutionError,
    primitives::header::BlsPublicKey,
};
use alloy_eips::eip7685::{Requests, RequestsOrHash};
use alloy_primitives::B256;
use alloy_rpc_types::engine::{
    CancunPayloadFields, ExecutionPayload, ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3,
    ExecutionPayloadEnvelopeV4, ExecutionPayloadEnvelopeV5, ExecutionPayloadInputV2,
    ExecutionPayloadSidecar, ExecutionPayloadV1, PraguePayloadFields,
};
use reth::{
    api::{BuiltPayload, EngineTypes, NodePrimitives, PayloadTypes},
    core::primitives::SealedBlock,
};
use reth_payload_primitives::ExecutionPayload as ExecutionPayloadTrait;

/// Berachain engine types configuration
///
/// This type defines the engine-specific types used by Berachain, including
/// payload attributes, built payload types, and execution data formats.
/// It delegates most functionality to Ethereum types while providing
/// extension points for Berachain-specific features.
#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct BerachainEngineTypes;

impl PayloadTypes for BerachainEngineTypes {
    type ExecutionData = BerachainExecutionData;

    type BuiltPayload = BerachainBuiltPayload;
    type PayloadAttributes = BerachainPayloadAttributes;
    type PayloadBuilderAttributes = BerachainPayloadBuilderAttributes;

    fn block_to_payload(
        block: SealedBlock<
            <<Self::BuiltPayload as BuiltPayload>::Primitives as NodePrimitives>::Block,
        >,
    ) -> Self::ExecutionData {
        let prev_proposer_pubkey = block.prev_proposer_pubkey;
        let (payload, sidecar) =
            ExecutionPayload::from_block_unchecked(block.hash(), &block.into_block());
        BerachainExecutionData::new(
            payload,
            BerachainExecutionPayloadSidecar {
                inner: sidecar,
                parent_proposer_pub_key: prev_proposer_pubkey,
            },
        )
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    derive_more::Deref,
    derive_more::DerefMut,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub struct BerachainExecutionPayloadEnvelopeV4 {
    /// Inner [`ExecutionPayloadEnvelopeV3`].
    #[deref]
    #[deref_mut]
    #[serde(flatten)]
    pub envelope_inner: ExecutionPayloadEnvelopeV3,

    /// A list of opaque [EIP-7685][eip7685] requests.
    ///
    /// [eip7685]: https://eips.ethereum.org/EIPS/eip-7685
    pub execution_requests: Requests,
    /// Introduced in BRIP-0004
    pub parent_proposer_pub_key: Option<BlsPublicKey>,
}

impl EngineTypes for BerachainEngineTypes {
    type ExecutionPayloadEnvelopeV1 = ExecutionPayloadV1;
    type ExecutionPayloadEnvelopeV2 = ExecutionPayloadEnvelopeV2;
    type ExecutionPayloadEnvelopeV3 = ExecutionPayloadEnvelopeV3;
    type ExecutionPayloadEnvelopeV4 = ExecutionPayloadEnvelopeV4;
    type ExecutionPayloadEnvelopeV5 = ExecutionPayloadEnvelopeV5;
}

/// Berachain-specific Prague payload fields that extend the standard Prague fields
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BerachainPraguePayloadFields {
    /// EIP-7685 requests
    pub requests: RequestsOrHash,
    /// Berachain-specific: Parent proposer public key (BRIP-0004)
    pub parent_proposer_pub_key: Option<BlsPublicKey>,
}

impl BerachainPraguePayloadFields {
    /// Create new Berachain Prague payload fields
    pub fn new(requests: RequestsOrHash, parent_proposer_pub_key: Option<BlsPublicKey>) -> Self {
        Self { requests, parent_proposer_pub_key }
    }
}

/// Berachain-specific ExecutionPayloadSidecar that extends the standard sidecar
/// with additional fields for Berachain consensus requirements
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BerachainExecutionPayloadSidecar {
    /// Standard ExecutionPayloadSidecar for compatibility
    #[serde(flatten)]
    pub inner: ExecutionPayloadSidecar,
    /// Berachain-specific: Parent proposer public key (BRIP-0004)
    pub parent_proposer_pub_key: Option<BlsPublicKey>,
}

impl BerachainExecutionPayloadSidecar {
    /// Creates a new instance with no additional fields (pre-Cancun)
    pub fn none() -> Self {
        Self { inner: ExecutionPayloadSidecar::none(), parent_proposer_pub_key: None }
    }

    /// Creates a new instance for Cancun (v3)
    pub fn v3(cancun: CancunPayloadFields) -> Self {
        Self { inner: ExecutionPayloadSidecar::v3(cancun), parent_proposer_pub_key: None }
    }

    /// Creates a new instance for Prague (v4) with Berachain-specific fields
    pub fn v4(
        cancun: CancunPayloadFields,
        requests: RequestsOrHash,
        parent_proposer_pub_key: Option<BlsPublicKey>,
    ) -> Self {
        Self {
            inner: ExecutionPayloadSidecar::v4(cancun, PraguePayloadFields { requests }),
            parent_proposer_pub_key,
        }
    }

    /// Returns the parent proposer public key if present
    pub fn parent_proposer_pub_key(&self) -> Option<BlsPublicKey> {
        self.parent_proposer_pub_key
    }

    /// Returns the EIP-7685 requests if available
    pub fn requests(&self) -> Option<&alloy_eips::eip7685::Requests> {
        self.inner.requests()
    }

    /// Returns the parent beacon block root if available
    pub fn parent_beacon_block_root(&self) -> Option<B256> {
        self.inner.parent_beacon_block_root()
    }

    /// Returns the versioned hashes if available
    pub fn versioned_hashes(&self) -> Option<&Vec<B256>> {
        self.inner.versioned_hashes()
    }

    /// Convert to standard ExecutionPayloadSidecar for compatibility
    pub fn into_inner(self) -> ExecutionPayloadSidecar {
        self.inner
    }

    /// Get reference to inner ExecutionPayloadSidecar
    pub fn inner(&self) -> &ExecutionPayloadSidecar {
        &self.inner
    }
}

/// Berachain-specific ExecutionData that uses BerachainExecutionPayloadSidecar
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BerachainExecutionData {
    /// The execution payload
    pub payload: ExecutionPayload,
    /// Berachain-specific sidecar with additional fields
    pub sidecar: BerachainExecutionPayloadSidecar,
}

impl BerachainExecutionData {
    /// Create new BerachainExecutionData
    pub fn new(payload: ExecutionPayload, sidecar: BerachainExecutionPayloadSidecar) -> Self {
        Self { payload, sidecar }
    }
}

impl ExecutionPayloadTrait for BerachainExecutionData {
    fn parent_hash(&self) -> B256 {
        self.payload.parent_hash()
    }

    fn block_hash(&self) -> B256 {
        self.payload.block_hash()
    }

    fn block_number(&self) -> u64 {
        self.payload.block_number()
    }

    fn withdrawals(&self) -> Option<&Vec<alloy_eips::eip4895::Withdrawal>> {
        self.payload.withdrawals()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.sidecar.parent_beacon_block_root()
    }

    fn timestamp(&self) -> u64 {
        self.payload.timestamp()
    }

    fn gas_used(&self) -> u64 {
        self.payload.as_v1().gas_used
    }
}

impl From<ExecutionPayloadV1> for BerachainExecutionData {
    fn from(payload: ExecutionPayloadV1) -> Self {
        Self { payload: payload.into(), sidecar: BerachainExecutionPayloadSidecar::none() }
    }
}

impl From<ExecutionPayloadInputV2> for BerachainExecutionData {
    fn from(payload: ExecutionPayloadInputV2) -> Self {
        Self { payload: payload.into_payload(), sidecar: BerachainExecutionPayloadSidecar::none() }
    }
}

/// Validates that proposer pubkey is present after Prague1 and absent before Prague1
pub fn validate_proposer_pubkey_prague1<ChainSpec: BerachainHardforks>(
    chain_spec: &ChainSpec,
    timestamp: u64,
    proposer_pub_key: Option<BlsPublicKey>,
) -> Result<(), BerachainExecutionError> {
    if chain_spec.is_prague1_active_at_timestamp(timestamp) {
        if proposer_pub_key.is_none() {
            return Err(BerachainExecutionError::MissingProposerPubkey);
        }
    } else if proposer_pub_key.is_some() {
        return Err(BerachainExecutionError::ProposerPubkeyNotAllowed);
    }
    Ok(())
}
