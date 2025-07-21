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
    /// Inner Ethereum beacon consensus implementation
    inner: EthBeaconConsensus<BerachainChainSpec>,
    chain_spec: Arc<BerachainChainSpec>,
}

impl BerachainBeaconConsensus {
    pub fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { inner: EthBeaconConsensus::new(chain_spec.clone()), chain_spec }
    }

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

        let base_fee = header.base_fee_per_gas.unwrap_or(0);

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
        primitives::header::BlsPublicKey,
        transaction::pol::{create_pol_transaction, validate_pol_transaction},
    };
    use alloy_primitives::U256;
    use reth_chainspec::EthChainSpec;
    use std::sync::Arc;

    fn mock_berachain_chainspec() -> Arc<BerachainChainSpec> {
        Arc::new(BerachainChainSpec::default())
    }

    fn mock_bls_pubkey() -> BlsPublicKey {
        BlsPublicKey::from([1u8; 48])
    }

    #[test]
    fn test_consensus_creation() {
        let chain_spec = mock_berachain_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec);

        assert_eq!(consensus.chain_spec.chain_id(), 1);
    }

    #[test]
    fn test_pol_transaction_creation_and_validation() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(10);
        let base_fee = 1000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, block_number, base_fee);

        assert!(pol_tx_envelope.is_ok(), "PoL transaction creation should succeed");

        let pol_tx = match pol_tx_envelope.unwrap() {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec.clone(), pubkey, block_number, base_fee);

        assert!(validation_result.is_ok(), "Valid PoL transaction should pass validation");
    }

    #[test]
    fn test_pol_transaction_validation_wrong_pubkey() {
        let chain_spec = mock_berachain_chainspec();
        let correct_pubkey = mock_bls_pubkey();
        let wrong_pubkey = BlsPublicKey::from([2u8; 48]);
        let block_number = U256::from(10);
        let base_fee = 1000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), correct_pubkey, block_number, base_fee)
                .unwrap();

        let pol_tx = match pol_tx_envelope {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec, wrong_pubkey, block_number, base_fee);

        assert!(
            validation_result.is_err(),
            "PoL transaction with wrong pubkey should fail validation"
        );
        assert!(validation_result.unwrap_err().to_string().contains("hash mismatch"));
    }

    #[test]
    fn test_pol_transaction_validation_wrong_base_fee() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(10);
        let correct_base_fee = 1000u64;
        let wrong_base_fee = 2000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, block_number, correct_base_fee)
                .unwrap();

        let pol_tx = match pol_tx_envelope {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec, pubkey, block_number, wrong_base_fee);

        assert!(
            validation_result.is_err(),
            "PoL transaction with wrong base fee should fail validation"
        );
        assert!(validation_result.unwrap_err().to_string().contains("hash mismatch"));
    }

    #[test]
    fn test_pol_transaction_validation_wrong_block_number() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let correct_block_number = U256::from(10);
        let wrong_block_number = U256::from(20);
        let base_fee = 1000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, correct_block_number, base_fee)
                .unwrap();

        let pol_tx = match pol_tx_envelope {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec, pubkey, wrong_block_number, base_fee);

        assert!(
            validation_result.is_err(),
            "PoL transaction with wrong block number should fail validation"
        );
        assert!(validation_result.unwrap_err().to_string().contains("hash mismatch"));
    }

    #[test]
    fn test_pol_transaction_deterministic_hashes() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(42);
        let base_fee = 1337u64;

        let pol_tx1_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, block_number, base_fee).unwrap();

        let pol_tx2_envelope =
            create_pol_transaction(chain_spec, pubkey, block_number, base_fee).unwrap();

        let pol_tx1 = match pol_tx1_envelope {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let pol_tx2 = match pol_tx2_envelope {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        assert_eq!(
            pol_tx1.hash(),
            pol_tx2.hash(),
            "Identical PoL transactions should have identical hashes"
        );
    }
}
