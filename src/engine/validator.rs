//! Berachain engine validation components

use crate::{
    chainspec::BerachainChainSpec,
    engine::{
        BerachainEngineTypes, BerachainExecutionData, BerachainExecutionPayloadSidecar,
        payload::BerachainPayloadAttributes,
    },
    hardforks::BerachainHardforks,
    primitives::{BerachainBlock, BerachainHeader, BerachainPrimitives},
    transaction::BerachainTxEnvelope,
};
use reth_engine_primitives::{EngineValidator, PayloadValidator};
use reth_ethereum_payload_builder::EthereumExecutionPayloadValidator;
use reth_node_api::{AddOnsContext, FullNodeComponents, NodeTypes, PayloadTypes};
use reth_node_builder::rpc::EngineValidatorBuilder;
use reth_payload_primitives::{
    EngineApiMessageVersion, EngineObjectValidationError, NewPayloadError, PayloadOrAttributes,
    validate_execution_requests, validate_version_specific_fields,
};
use reth_primitives_traits::{Block, RecoveredBlock, SealedBlock};
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

    /// Parse the execution payload into a BerachainBlock
    fn parse_berachain_block(
        &self,
        payload: alloy_rpc_types::engine::ExecutionPayload,
        sidecar: &BerachainExecutionPayloadSidecar,
    ) -> Result<SealedBlock<BerachainBlock>, NewPayloadError> {
        // Use the standard try_into_block_with_sidecar method to parse the block
        let standard_block = payload
            .try_into_block_with_sidecar::<BerachainTxEnvelope>(&sidecar.inner)
            .map_err(|e| NewPayloadError::Other(e.into()))?;

        // Convert header from standard to BerachainHeader
        let mut berachain_header = BerachainHeader::from(standard_block.header.clone());

        berachain_header.prev_proposer_pubkey = sidecar.parent_proposer_pub_key;

        // Create BerachainBlock with converted header and body
        let berachain_ommers: Vec<BerachainHeader> =
            standard_block.body.ommers.iter().map(|h| BerachainHeader::from(h.clone())).collect();
        let berachain_body: alloy_consensus::BlockBody<BerachainTxEnvelope, BerachainHeader> =
            alloy_consensus::BlockBody {
                transactions: standard_block.body.transactions.clone(),
                ommers: berachain_ommers,
                withdrawals: standard_block.body.withdrawals.clone(),
            };
        let berachain_block =
            alloy_consensus::Block { header: berachain_header, body: berachain_body };

        Ok(berachain_block.seal_slow())
    }

    /// Validate hardfork-specific fields
    fn validate_hardfork_fields(
        &self,
        _sealed_block: &SealedBlock<BerachainBlock>,
        _sidecar: &BerachainExecutionPayloadSidecar,
    ) -> Result<(), NewPayloadError> {
        // For simplicity, we'll skip the standard hardfork validations here
        // since they expect standard headers. The inner validator already
        // validated the standard block, so we just need to do Berachain-specific
        // validation in validate_berachain_specific_fields

        // TODO: Implement proper hardfork validation for BerachainBlock
        // This would involve creating Berachain-specific versions of the
        // validation functions that work with BerachainHeader

        Ok(())
    }

    /// Validate Berachain-specific fields including PoL transaction rules
    fn validate_berachain_specific_fields(
        &self,
        sealed_block: &SealedBlock<BerachainBlock>,
    ) -> Result<(), NewPayloadError> {
        let transactions: Vec<&BerachainTxEnvelope> = sealed_block.body().transactions().collect();
        let header = sealed_block.header();
        let is_prague1_active = self.chain_spec().is_prague1_active_at_timestamp(header.timestamp);

        if transactions.is_empty() {
            // After Prague1, blocks must contain at least the PoL transaction
            if is_prague1_active {
                return Err(NewPayloadError::Other(
                    "Block must contain at least one PoL transaction after Prague1 hardfork".into(),
                ));
            }
            // Before Prague1, empty blocks are valid
            return Ok(());
        }

        // PoL transaction rules only apply after Prague1 activation
        if is_prague1_active {
            // Rule 1: The first transaction must be a PoL transaction. Guaranteed at least 1 tx
            // due to empty check beforehand.
            let first_tx = transactions[0];
            if !self.is_pol_transaction(first_tx) {
                return Err(NewPayloadError::Other(
                    "First transaction must be a PoL transaction".into(),
                ));
            }

            // Rule 2: No other transaction should be a PoL transaction
            for (index, tx) in transactions.iter().enumerate().skip(1) {
                if self.is_pol_transaction(tx) {
                    return Err(NewPayloadError::Other(
                        format!(
                            "PoL transaction found at index {index} but only allowed at index 0"
                        )
                        .into(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Check if a transaction is a PoL transaction
    fn is_pol_transaction(&self, tx: &BerachainTxEnvelope) -> bool {
        matches!(tx, BerachainTxEnvelope::Berachain(_))
    }
}

impl PayloadValidator for BerachainEngineValidator {
    type Block = BerachainBlock;
    type ExecutionData = BerachainExecutionData;

    fn ensure_well_formed_payload(
        &self,
        payload: BerachainExecutionData,
    ) -> Result<RecoveredBlock<Self::Block>, NewPayloadError> {
        let BerachainExecutionData { payload, sidecar } = payload;
        let expected_hash = payload.block_hash();

        // Parse the block directly to BerachainBlock
        let sealed_block = self.parse_berachain_block(payload, &sidecar)?;

        // Validate block hash
        if expected_hash != sealed_block.hash() {
            return Err(NewPayloadError::Other(
                format!(
                    "Block hash mismatch: expected {}, got {}",
                    expected_hash,
                    sealed_block.hash()
                )
                .into(),
            ));
        }

        // Apply standard hardfork validations
        self.validate_hardfork_fields(&sealed_block, &sidecar)?;

        // Apply Berachain-specific validations
        self.validate_berachain_specific_fields(&sealed_block)?;

        sealed_block.try_recover().map_err(|e| NewPayloadError::Other(e.into()))
    }
}

impl<Types> EngineValidator<Types> for BerachainEngineValidator
where
    Types: PayloadTypes<
            PayloadAttributes = BerachainPayloadAttributes,
            ExecutionData = BerachainExecutionData,
        >,
{
    fn validate_version_specific_fields(
        &self,
        version: EngineApiMessageVersion,
        payload_or_attrs: PayloadOrAttributes<'_, Self::ExecutionData, BerachainPayloadAttributes>,
    ) -> Result<(), EngineObjectValidationError> {
        // Extract execution requests from the payload if present
        let execution_requests =
            if let PayloadOrAttributes::ExecutionPayload(payload) = &payload_or_attrs {
                payload.sidecar.requests()
            } else {
                None
            };

        // Validate execution requests if present
        if let Some(requests) = execution_requests {
            validate_execution_requests(requests)?;
        }

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
            Payload = BerachainEngineTypes,
            Primitives = BerachainPrimitives,
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
