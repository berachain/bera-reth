use crate::{
    chainspec::BerachainChainSpec,
    hardforks::BerachainHardforks,
    primitives::{BerachainBlock, BerachainHeader, BerachainPrimitives},
    transaction::{BerachainTxEnvelope, pol::validate_pol_transaction},
};
use reth::{
    api::NodeTypes,
    beacon_consensus::EthBeaconConsensus,
    consensus::{Consensus, ConsensusError, FullConsensus, HeaderValidator},
    providers::BlockExecutionResult,
};
use reth_node_api::FullNodeTypes;
use reth_node_builder::{BuilderContext, components::ConsensusBuilder};
use reth_primitives_traits::{NodePrimitives, RecoveredBlock, SealedBlock, SealedHeader};
use std::{fmt::Debug, sync::Arc};

#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainConsensusBuilder;

impl<Node> ConsensusBuilder<Node> for BerachainConsensusBuilder
where
    Node: FullNodeTypes<
        Types: NodeTypes<ChainSpec = BerachainChainSpec, Primitives = BerachainPrimitives>,
    >,
{
    type Consensus = Arc<dyn FullConsensus<BerachainPrimitives, Error = ConsensusError>>;

    async fn build_consensus(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::Consensus> {
        Ok(Arc::new(BerachainBeaconConsensus::new(ctx.chain_spec())))
    }
}

#[derive(Debug, Clone)]
pub struct BerachainBeaconConsensus {
    inner: EthBeaconConsensus<BerachainChainSpec>,
    chain_spec: Arc<BerachainChainSpec>,
}

impl BerachainBeaconConsensus {
    pub fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { inner: EthBeaconConsensus::new(chain_spec.clone()), chain_spec }
    }

    /// Will ensure the PoL transaction is the first tx in the block and has the correct hash
    fn validate_pol_transaction(
        &self,
        block: &SealedBlock<BerachainBlock>,
    ) -> Result<(), ConsensusError> {
        let transactions: Vec<_> = block.body().transactions().collect();

        if transactions.is_empty() {
            return Err(ConsensusError::Other(
                "Prague1 block must contain at least one PoL transaction".into(),
            ));
        }

        // Check first transaction is PoL and validate its shape
        let first_tx = &transactions[0];
        if let BerachainTxEnvelope::Berachain(pol_tx) = first_tx {
            self.validate_pol_transaction_shape(pol_tx, block)?;
        } else {
            return Err(ConsensusError::Other(
                "First transaction in Prague1 block must be a PoL transaction".into(),
            ));
        }

        // Check no other transactions are PoL
        for (index, tx) in transactions.iter().enumerate().skip(1) {
            if matches!(tx, BerachainTxEnvelope::Berachain(_)) {
                return Err(ConsensusError::Other(format!(
                    "PoL transaction found at invalid position {index}, only first transaction can be PoL"
                )));
            }
        }

        Ok(())
    }

    fn validate_pol_transaction_shape(
        &self,
        pol_tx: &alloy_primitives::Sealed<crate::transaction::PoLTx>,
        block: &SealedBlock<BerachainBlock>,
    ) -> Result<(), ConsensusError> {
        let header = block.header();

        let expected_pubkey = header.prev_proposer_pubkey.ok_or_else(|| {
            ConsensusError::Other(
                "Block header missing prev_proposer_pubkey for PoL transaction validation".into(),
            )
        })?;

        let base_fee = header
            .base_fee_per_gas
            .ok_or_else(|| ConsensusError::Other("Base fee must be present in header".into()))?;

        validate_pol_transaction(
            pol_tx,
            self.chain_spec.clone(),
            expected_pubkey,
            alloy_primitives::U256::from(header.number),
            base_fee,
        )
    }
}

impl FullConsensus<BerachainPrimitives> for BerachainBeaconConsensus {
    fn validate_block_post_execution(
        &self,
        block: &RecoveredBlock<BerachainBlock>,
        result: &BlockExecutionResult<<BerachainPrimitives as NodePrimitives>::Receipt>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<BerachainChainSpec> as FullConsensus<BerachainPrimitives>>::validate_block_post_execution(&self.inner, block, result)
    }
}

impl Consensus<BerachainBlock> for BerachainBeaconConsensus {
    type Error = ConsensusError;

    fn validate_body_against_header(
        &self,
        body: &<BerachainBlock as reth_primitives_traits::Block>::Body,
        header: &SealedHeader<BerachainHeader>,
    ) -> Result<(), Self::Error> {
        <EthBeaconConsensus<BerachainChainSpec> as Consensus<BerachainBlock>>::validate_body_against_header(
            &self.inner,
            body,
            header,
        )
    }

    fn validate_block_pre_execution(
        &self,
        block: &SealedBlock<BerachainBlock>,
    ) -> Result<(), Self::Error> {
        <EthBeaconConsensus<BerachainChainSpec> as Consensus<BerachainBlock>>::validate_block_pre_execution(
            &self.inner,
            block,
        )?;

        if self.chain_spec.is_prague1_active_at_timestamp(block.header().timestamp) {
            self.validate_pol_transaction(block)?;
        } else if let Some(index) = block
            .body()
            .transactions()
            .position(|tx| matches!(tx, BerachainTxEnvelope::Berachain(_)))
        {
            return Err(ConsensusError::Other(format!(
                "PoL transaction found at position {index} before Prague1 fork activation"
            )));
        }
        Ok(())
    }
}

impl HeaderValidator<BerachainHeader> for BerachainBeaconConsensus {
    fn validate_header(
        &self,
        header: &SealedHeader<BerachainHeader>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<BerachainChainSpec> as HeaderValidator<BerachainHeader>>::validate_header(
            &self.inner,
            header,
        )
    }

    fn validate_header_against_parent(
        &self,
        header: &SealedHeader<BerachainHeader>,
        parent: &SealedHeader<BerachainHeader>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<BerachainChainSpec> as HeaderValidator<BerachainHeader>>::validate_header_against_parent(&self.inner, header, parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chainspec::BerachainChainSpec,
        primitives::{BerachainBlockBody, BerachainHeader, header::BlsPublicKey},
        transaction::{BerachainTxEnvelope, pol::create_pol_transaction},
    };
    use alloy_consensus::{EMPTY_OMMER_ROOT_HASH, Signed, TxLegacy, constants::EMPTY_WITHDRAWALS};
    use alloy_eips::eip4895::Withdrawals;
    use alloy_primitives::{Address, BlockHash, TxKind, U256};
    use reth_primitives_traits::{BlockBody, SealedBlock, SealedHeader};
    use std::sync::Arc;

    fn mock_berachain_chainspec() -> Arc<BerachainChainSpec> {
        crate::test::bepolia_chainspec()
    }

    fn mock_bls_pubkey() -> BlsPublicKey {
        BlsPublicKey::from([1u8; 48])
    }

    #[test]
    fn test_pre_prague1_pol_transaction_rejected() {
        let chain_spec = mock_berachain_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(10);
        let base_fee = 1000u64;

        // Verify Prague1 activation timestamp for context
        assert!(
            !chain_spec.is_prague1_active_at_timestamp(0),
            "Timestamp 0 should be before Prague1 activation"
        );

        // Create a PoL transaction
        let pol_tx_envelope =
            create_pol_transaction(chain_spec, pubkey, block_number, base_fee).unwrap();

        // Create a block body with the PoL transaction
        let transactions = vec![pol_tx_envelope];
        let block_body = BerachainBlockBody {
            transactions: transactions.clone(),
            withdrawals: Some(Withdrawals::default()),
            ..Default::default()
        };

        // Create a header with timestamp BEFORE Prague1 activation
        let header = BerachainHeader {
            number: block_number.to::<u64>(),
            timestamp: 0, // Pre-Prague1 timestamp (Prague1 activates at 1754496000)
            base_fee_per_gas: Some(base_fee),
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            transactions_root: block_body.calculate_tx_root(),
            withdrawals_root: Some(EMPTY_WITHDRAWALS),
            blob_gas_used: Some(0),
            ..Default::default()
        };

        let sealed_header = SealedHeader::new(header, BlockHash::ZERO);
        let block = SealedBlock::from_sealed_parts(sealed_header, block_body);

        // Validation should fail because PoL transaction exists before Prague1
        let result = consensus.validate_block_pre_execution(&block);
        assert!(result.is_err(), "Pre-Prague1 block with PoL transaction should fail validation");

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("before Prague1 fork activation"),
            "Error should mention Prague1 fork activation"
        );
        assert!(error_msg.contains("position 0"), "Error should indicate PoL transaction position");
    }

    #[test]
    fn test_pre_prague1_normal_transactions_accepted() {
        let chain_spec = mock_berachain_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());

        // Verify Prague1 activation timestamp for context
        assert!(
            !chain_spec.is_prague1_active_at_timestamp(0),
            "Timestamp 0 should be before Prague1 activation"
        );

        // Create normal Ethereum transaction
        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 21000,
            to: TxKind::Call(Address::ZERO),
            value: U256::ZERO,
            input: Default::default(),
        };

        let signature = alloy_primitives::Signature::test_signature();
        let signed_tx = Signed::new_unhashed(tx, signature);
        let eth_tx_envelope =
            BerachainTxEnvelope::Ethereum(alloy_consensus::TxEnvelope::Legacy(signed_tx));

        let transactions = vec![eth_tx_envelope];
        let block_body = BerachainBlockBody {
            transactions: transactions.clone(),
            withdrawals: Some(Withdrawals::default()),
            ..Default::default()
        };

        let header = BerachainHeader {
            number: 10,
            timestamp: 0, // Pre-Prague1 timestamp
            base_fee_per_gas: Some(1000),
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            transactions_root: block_body.calculate_tx_root(),
            withdrawals_root: Some(EMPTY_WITHDRAWALS),
            blob_gas_used: Some(0),
            ..Default::default()
        };

        let sealed_header = SealedHeader::new(header, BlockHash::ZERO);
        let block = SealedBlock::from_sealed_parts(sealed_header, block_body);

        // Validation should succeed for normal transactions pre-Prague1
        let result = consensus.validate_block_pre_execution(&block);
        assert!(
            result.is_ok(),
            "Pre-Prague1 block with normal transactions should pass validation"
        );
    }
}
