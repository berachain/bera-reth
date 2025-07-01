//! Berachain engine validation components

use crate::{chainspec::BerachainChainSpec, engine::payload::BerachainPayloadAttributes};
use alloy_rpc_types::engine::ExecutionData;
use reth_engine_primitives::{EngineTypes, EngineValidator, PayloadValidator};
use reth_ethereum_payload_builder::EthereumExecutionPayloadValidator;
use reth_ethereum_primitives::{Block, EthPrimitives};
use reth_node_api::{AddOnsContext, FullNodeComponents, NodeTypes, PayloadTypes};
use reth_node_builder::rpc::EngineValidatorBuilder;
use reth_payload_primitives::{
    EngineApiMessageVersion, EngineObjectValidationError, NewPayloadError, PayloadOrAttributes,
    validate_execution_requests, validate_version_specific_fields,
};
use reth_primitives_traits::RecoveredBlock;
use std::{marker::PhantomData, sync::Arc};

#[derive(Debug, Clone)]
pub struct BerachainEngineValidator {
    inner: EthereumExecutionPayloadValidator<BerachainChainSpec>,
}

impl BerachainEngineValidator {
    /// Instantiates a new validator.
    pub const fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { inner: EthereumExecutionPayloadValidator::new(chain_spec) }
    }

    /// Returns the chain spec used by the validator.
    #[inline]
    fn chain_spec(&self) -> &BerachainChainSpec {
        self.inner.chain_spec()
    }
}

impl PayloadValidator for BerachainEngineValidator {
    type Block = Block;
    type ExecutionData = ExecutionData;

    fn ensure_well_formed_payload(
        &self,
        payload: ExecutionData,
    ) -> Result<RecoveredBlock<Self::Block>, NewPayloadError> {
        let sealed_block = self.inner.ensure_well_formed_payload(payload)?;
        sealed_block.try_recover().map_err(|e| NewPayloadError::Other(e.into()))
    }
}

impl<Types> EngineValidator<Types> for BerachainEngineValidator
where
    Types:
        PayloadTypes<PayloadAttributes = BerachainPayloadAttributes, ExecutionData = ExecutionData>,
{
    fn validate_version_specific_fields(
        &self,
        version: EngineApiMessageVersion,
        payload_or_attrs: PayloadOrAttributes<'_, Self::ExecutionData, BerachainPayloadAttributes>,
    ) -> Result<(), EngineObjectValidationError> {
        payload_or_attrs
            .execution_requests()
            .map(|requests| validate_execution_requests(requests))
            .transpose()?;

        validate_version_specific_fields(self.chain_spec(), version, payload_or_attrs)
    }

    fn ensure_well_formed_attributes(
        &self,
        version: EngineApiMessageVersion,
        attributes: &BerachainPayloadAttributes,
    ) -> Result<(), EngineObjectValidationError> {
        validate_version_specific_fields(
            self.chain_spec(),
            version,
            PayloadOrAttributes::<Self::ExecutionData, BerachainPayloadAttributes>::PayloadAttributes(
                attributes,
            ),
        )
    }
}

/// Builder for BerachainEngineValidator that works with BerachainPayloadAttributes
#[derive(Debug, Default, Clone)]
pub struct BerachainEngineValidatorBuilder {
    _phantom: PhantomData<BerachainChainSpec>,
}

impl<Node, Types> EngineValidatorBuilder<Node> for BerachainEngineValidatorBuilder
where
    Types: NodeTypes<
            ChainSpec = BerachainChainSpec,
            Payload: EngineTypes<ExecutionData = ExecutionData>
                         + PayloadTypes<PayloadAttributes = BerachainPayloadAttributes>,
            Primitives = EthPrimitives,
        >,
    Node: FullNodeComponents<Types = Types>,
{
    type Validator = BerachainEngineValidator;

    async fn build(self, ctx: &AddOnsContext<'_, Node>) -> eyre::Result<Self::Validator> {
        Ok(BerachainEngineValidator::new(ctx.config.chain.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_chainspec::EthChainSpec;

    fn create_test_chain_spec() -> Arc<BerachainChainSpec> {
        let mut genesis = alloy_genesis::Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(alloy_primitives::U256::ZERO);
        Arc::new(BerachainChainSpec::from(genesis))
    }

    #[test]
    fn test_berachain_engine_validator_new() {
        let chain_spec = create_test_chain_spec();
        let validator = BerachainEngineValidator::new(chain_spec.clone());

        assert_eq!(validator.chain_spec().chain().id(), chain_spec.chain().id());
    }

    #[test]
    fn test_chain_spec_access() {
        let chain_spec = create_test_chain_spec();
        let expected_chain_id = chain_spec.chain().id();
        let validator = BerachainEngineValidator::new(chain_spec);

        assert_eq!(validator.chain_spec().chain().id(), expected_chain_id);
    }
}
