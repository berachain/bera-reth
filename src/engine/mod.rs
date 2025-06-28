//! Berachain engine types and validation

pub mod builder;
mod payload;
pub mod validator;

use crate::engine::payload::{BeraPayloadAttributes, BeraPayloadBuilderAttributes};
use alloy_rpc_types::engine::{
    ExecutionData, ExecutionPayload, ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3,
    ExecutionPayloadEnvelopeV4, ExecutionPayloadEnvelopeV5, ExecutionPayloadV1,
};
use reth::{
    api::{BuiltPayload, EngineTypes, NodePrimitives, PayloadTypes},
    core::primitives::SealedBlock,
};
use reth_node_ethereum::EthEngineTypes;

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct BeraEngineTypes;

impl PayloadTypes for BeraEngineTypes {
    type ExecutionData = <EthEngineTypes as PayloadTypes>::ExecutionData;
    type BuiltPayload = <EthEngineTypes as PayloadTypes>::BuiltPayload;
    type PayloadAttributes = BeraPayloadAttributes;
    type PayloadBuilderAttributes = BeraPayloadBuilderAttributes;

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

impl EngineTypes for BeraEngineTypes {
    type ExecutionPayloadEnvelopeV1 = ExecutionPayloadV1;
    type ExecutionPayloadEnvelopeV2 = ExecutionPayloadEnvelopeV2;
    type ExecutionPayloadEnvelopeV3 = ExecutionPayloadEnvelopeV3;
    type ExecutionPayloadEnvelopeV4 = ExecutionPayloadEnvelopeV4;
    type ExecutionPayloadEnvelopeV5 = ExecutionPayloadEnvelopeV5;
}
