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
        } else {
            if let Some(index) = block
                .body()
                .transactions()
                .position(|tx| matches!(tx, BerachainTxEnvelope::Berachain(_)))
            {
                return Err(ConsensusError::Other(format!(
                    "PoL transaction found at position {index} before Prague1 fork activation"
                )));
            }
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
    use alloy_consensus::EMPTY_OMMER_ROOT_HASH;
    use alloy_primitives::{U256, hex::FromHex};
    use reth_chainspec::EthChainSpec;
    use reth_primitives_traits::BlockBody;
    use std::sync::Arc;

    fn mock_berachain_chainspec() -> Arc<BerachainChainSpec> {
        Arc::new(BerachainChainSpec::default())
    }

    fn mock_bls_pubkey() -> BlsPublicKey {
        BlsPublicKey::from([1u8; 48])
    }

    #[test]
    // TODO: Move this to pol.rs as this is not a consensus tests
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
    // TODO: Move this to pol.rs as this is not a consensus tests
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
    // TODO: Move this to pol.rs as this is not a consensus tests
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
    // TODO: Move this to pol.rs as this is not a consensus tests
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
    // TODO: Move this to pol.rs as this is not a consensus tests
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

    #[test]
    fn test_pre_prague1_pol_transaction_rejected() {
        use crate::primitives::BerachainBlockBody;
        use alloy_primitives::BlockHash;
        use reth_primitives_traits::{SealedBlock, SealedHeader};

        let chain_spec = mock_berachain_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(10);
        let base_fee = 1000u64;

        // Verify Prague1 activation timestamp for context
        println!(
            "Prague1 activation timestamp: {:?}",
            chain_spec.is_prague1_active_at_timestamp(0)
        );
        assert!(
            !chain_spec.is_prague1_active_at_timestamp(0),
            "Timestamp 0 should be before Prague1 activation"
        );

        // Create a PoL transaction
        let pol_tx_envelope =
            create_pol_transaction(chain_spec, pubkey, block_number, base_fee).unwrap();

        // Create a block body with the PoL transaction
        let transactions = vec![pol_tx_envelope];
        let mut block_body = BerachainBlockBody::default();
        block_body.transactions = transactions.clone();

        // Create a header with timestamp BEFORE Prague1 activation (assuming Prague1 activates
        // later)
        let mut header = BerachainHeader::default();
        header.number = block_number.to::<u64>();
        header.timestamp = 0; // Pre-Prague1 timestamp
        header.base_fee_per_gas = Some(base_fee);
        header.ommers_hash = EMPTY_OMMER_ROOT_HASH;
        header.transactions_root = block_body.calculate_tx_root();

        let sealed_header = SealedHeader::new(header, BlockHash::ZERO);
        let block = SealedBlock::from_sealed_parts(sealed_header, block_body);

        // Validation should fail because PoL transaction exists before Prague1
        let result = consensus.validate_block_pre_execution(&block);
        println!("Validation result: {:?}", result);

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
        use crate::primitives::BerachainBlockBody;
        use alloy_consensus::{Signed, TxLegacy};
        use alloy_primitives::{Address, BlockHash, TxKind};
        use reth_primitives_traits::{SealedBlock, SealedHeader};

        let chain_spec = mock_berachain_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec);

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
        let eth_tx_envelope = crate::transaction::BerachainTxEnvelope::Ethereum(
            alloy_consensus::TxEnvelope::Legacy(signed_tx),
        );

        let transactions = vec![eth_tx_envelope];
        let mut block_body = BerachainBlockBody::default();
        block_body.transactions = transactions.clone();

        let mut header = crate::primitives::BerachainHeader::default();
        header.number = 10;
        header.timestamp = 0; // Pre-Prague1 timestamp
        header.base_fee_per_gas = Some(1000);
        header.ommers_hash = alloy_primitives::B256::from_hex(
            "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
        )
        .unwrap();
        header.transactions_root = block_body.calculate_tx_root();

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
