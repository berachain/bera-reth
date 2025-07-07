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
pub mod validator;

use crate::engine::{
    builder::BerachainPayloadBuilder,
    payload::{
        BerachainBuiltPayload, BerachainPayloadAttributes, BerachainPayloadBuilderAttributes,
    },
};
use alloy_rpc_types::engine::{
    ExecutionData, ExecutionPayload, ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3,
    ExecutionPayloadEnvelopeV4, ExecutionPayloadEnvelopeV5, ExecutionPayloadV1,
};
use reth::{
    api::{BuiltPayload, EngineTypes, NodePrimitives, PayloadTypes},
    core::primitives::SealedBlock,
};
use reth_ethereum_engine_primitives::{BuiltPayloadConversionError, EthBuiltPayload};
use reth_node_ethereum::EthEngineTypes;

/// Berachain engine types configuration
///
/// This type defines the engine-specific types used by Berachain, including
/// payload attributes, built payload types, and execution data formats.
/// It delegates most functionality to Ethereum types while providing
/// extension points for Berachain-specific features.
/// TODO: Add custom execution data types when Berachain-specific logic is needed.
#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct BerachainEngineTypes;

impl PayloadTypes for BerachainEngineTypes {
    type ExecutionData = <EthEngineTypes as PayloadTypes>::ExecutionData;

    // TODO: Change the built payload type to Berachain use BerachainPrimitives
    type BuiltPayload = BerachainBuiltPayload;
    type PayloadAttributes = BerachainPayloadAttributes;
    type PayloadBuilderAttributes = BerachainPayloadBuilderAttributes;

    fn block_to_payload(
        block: SealedBlock<
            <<Self::BuiltPayload as BuiltPayload>::Primitives as NodePrimitives>::Block,
        >,
    ) -> Self::ExecutionData {
        let (payload, sidecar) =
            ExecutionPayload::from_block_unchecked(block.hash(), &block.into_block());
        ExecutionData { payload, sidecar }
    }
}

impl EngineTypes for BerachainEngineTypes {
    type ExecutionPayloadEnvelopeV1 = ExecutionPayloadV1;
    type ExecutionPayloadEnvelopeV2 = ExecutionPayloadEnvelopeV2;
    type ExecutionPayloadEnvelopeV3 = ExecutionPayloadEnvelopeV3;
    type ExecutionPayloadEnvelopeV4 = ExecutionPayloadEnvelopeV4;
    type ExecutionPayloadEnvelopeV5 = ExecutionPayloadEnvelopeV5;
}
