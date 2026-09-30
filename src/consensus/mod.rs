use crate::{
    chainspec::BerachainChainSpec,
    hardforks::BerachainHardforks,
    node::evm::error::BerachainExecutionError,
    primitives::{BerachainBlock, BerachainHeader, BerachainPrimitives},
    transaction::{BerachainTxEnvelope, pol::validate_pol_transaction},
};
use alloy_consensus::BlockHeader;
use alloy_primitives::Address;
use reth::{
    api::NodeTypes,
    beacon_consensus::EthBeaconConsensus,
    consensus::{Consensus, ConsensusError, FullConsensus, HeaderValidator, ReceiptRootBloom},
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
    type Consensus = Arc<BerachainBeaconConsensus>;

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
        receipt_root_bloom: Option<ReceiptRootBloom>,
    ) -> Result<(), ConsensusError> {
        // First run the standard validation
        <EthBeaconConsensus<BerachainChainSpec> as FullConsensus<BerachainPrimitives>>::validate_block_post_execution(&self.inner, block, result, receipt_root_bloom)?;

        // ============================================================================
        // Prague3 historical-window validation
        // ----------------------------------------------------------------------------
        // The block below enforces the Prague3 emergency rules that were live on
        // Berachain mainnet for the timestamp window [1762164459, 1762963200) — from
        // 2025-11-03 to 2025-11-12, when Prague4 ended the restrictions.
        //
        // This code is NOT dead. It is gated on `is_prague3_active_at_timestamp`,
        // which returns true only inside that historical window. Outside the window
        // (i.e. on the live tip post-Prague4, and pre-Prague3) the gate short-circuits
        // and none of the loops run. The path remains a hard requirement for any node
        // re-executing the chain from genesis.
        //
        // Note on the three log checks in this module and `deposits.rs`:
        //   * Deposit parser (deposits.rs)        — filters by `log.address` because authority over
        //     `Deposit(...)` is contract-scoped to the deposit contract.
        //   * InternalBalanceChanged (below)      — filters by `log.address` because the event's
        //     semantic meaning is BEX-vault-internal accounting; another contract emitting the same
        //     topic is a hash-shape coincidence, not the same event.
        //   * Transfer (below)                    — intentionally does NOT filter by `log.address`.
        //     The rule blocks any ERC20 transfer involving a blocked address; ERC20 Transfer events
        //     are authored by each token contract, so scoping to a single address would defeat the
        //     rule.
        //
        // For bug-bounty triage: findings against this block targeting the live tip
        // are out of scope — Prague4 made the gate inactive on all production
        // timestamps.
        // ============================================================================
        let timestamp = block.header().timestamp();
        if let Some(blocked_addresses) =
            self.chain_spec.prague3_blocked_addresses_at_timestamp(timestamp)
        {
            let rescue_address = self.chain_spec.prague3_rescue_address_at_timestamp(timestamp);
            let bex_vault_address =
                self.chain_spec.prague3_bex_vault_address_at_timestamp(timestamp);

            // ERC20 Transfer event signature: Transfer(address,address,uint256)
            const TRANSFER_EVENT_SIGNATURE: alloy_primitives::B256 = alloy_primitives::b256!(
                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
            );

            // Check all receipts for ERC20 Transfer events involving blocked addresses or BEX vault
            for receipt in &result.receipts {
                for log in &receipt.logs {
                    // Check if this is a Transfer event (first topic is the event signature)
                    if log.topics().first() == Some(&TRANSFER_EVENT_SIGNATURE) &&
                        log.topics().len() >= 3
                    {
                        // Transfer event has indexed from (topics[1]) and to (topics[2]) addresses
                        let from_addr = Address::from_word(log.topics()[1]);
                        let to_addr = Address::from_word(log.topics()[2]);

                        // Check if BEX vault is involved in the transfer (block all BEX vault
                        // transfers)
                        if let Some(bex_vault) = bex_vault_address &&
                            (from_addr == bex_vault || to_addr == bex_vault)
                        {
                            return Err(ConsensusError::Other(
                                BerachainExecutionError::Prague3BexVaultTransfer {
                                    vault_address: bex_vault,
                                }
                                .to_string(),
                            ));
                        }

                        // Check if from address is blocked
                        if blocked_addresses.contains(&from_addr) {
                            // Blocked addresses can only send to rescue address
                            if rescue_address != Some(to_addr) {
                                return Err(ConsensusError::Other(
                                    BerachainExecutionError::Prague3BlockedAddressTransfer {
                                        blocked_address: from_addr,
                                    }
                                    .to_string(),
                                ));
                            }
                        }

                        // Check if to address is blocked (blocked addresses cannot receive)
                        if blocked_addresses.contains(&to_addr) {
                            return Err(ConsensusError::Other(
                                BerachainExecutionError::Prague3BlockedAddressTransfer {
                                    blocked_address: to_addr,
                                }
                                .to_string(),
                            ));
                        }
                    }
                }
            }
        }

        // Check for Prague3 BEX vault InternalBalanceChanged events if the hardfork is active
        if let Some(bex_vault_address) =
            self.chain_spec.prague3_bex_vault_address_at_timestamp(timestamp)
        {
            // InternalBalanceChanged event signature:
            // InternalBalanceChanged(address,address,int256)
            const INTERNAL_BALANCE_CHANGED_SIGNATURE: alloy_primitives::B256 = alloy_primitives::b256!(
                "18e1ea4139e68413d7d08aa752e71568e36b2c5bf940893314c2c5b01eaa0c42"
            );

            // Check all receipts for InternalBalanceChanged events from BEX vault
            for receipt in &result.receipts {
                for log in &receipt.logs {
                    // Check if this log is from the BEX vault and is an InternalBalanceChanged
                    // event
                    if log.address == bex_vault_address &&
                        log.topics().first() == Some(&INTERNAL_BALANCE_CHANGED_SIGNATURE)
                    {
                        return Err(ConsensusError::Other(
                            BerachainExecutionError::Prague3BexVaultEvent {
                                vault_address: bex_vault_address,
                            }
                            .to_string(),
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

impl Consensus<BerachainBlock> for BerachainBeaconConsensus {
    fn validate_body_against_header(
        &self,
        body: &<BerachainBlock as reth_primitives_traits::Block>::Body,
        header: &SealedHeader<BerachainHeader>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<BerachainChainSpec> as Consensus<BerachainBlock>>::validate_body_against_header(
            &self.inner,
            body,
            header,
        )
    }

    fn validate_block_pre_execution(
        &self,
        block: &SealedBlock<BerachainBlock>,
    ) -> Result<(), ConsensusError> {
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
        )?;

        // Enforce the Prague1 fork gate on `prev_proposer_pubkey` here so the import path
        // matches the Engine API and executor. Without this, peer-imported headers could
        // bypass the invariant and only get caught later at execution time.
        crate::engine::validate_proposer_pubkey_prague1(
            &*self.chain_spec,
            header.timestamp(),
            header.prev_proposer_pubkey,
        )
        .map_err(|err| ConsensusError::Other(err.to_string()))?;

        Ok(())
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
        primitives::{BerachainBlockBody, BerachainHeader, header::BlsPublicKey},
        transaction::{BerachainTxEnvelope, pol::create_pol_transaction},
    };
    use alloy_consensus::{EMPTY_OMMER_ROOT_HASH, Signed, TxLegacy, constants::EMPTY_WITHDRAWALS};
    use alloy_eips::eip4895::Withdrawals;
    use alloy_primitives::{Address, BlockHash, TxKind, U256};
    use reth_ethereum_primitives::TransactionSigned;
    use reth_primitives_traits::{BlockBody, SealedBlock, SealedHeader};

    use crate::test_utils::bepolia_chainspec;

    fn mock_bls_pubkey() -> BlsPublicKey {
        BlsPublicKey::from([1u8; 48])
    }

    #[test]
    fn test_pre_prague1_pol_transaction_rejected() {
        let chain_spec = bepolia_chainspec();
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
        let chain_spec = bepolia_chainspec();
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
        let eth_tx_envelope = BerachainTxEnvelope::Ethereum(TransactionSigned::Legacy(signed_tx));

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

    /// Build a header that satisfies upstream `EthBeaconConsensus::validate_header`
    /// for the given timestamp, so any failure in `BerachainBeaconConsensus::validate_header`
    /// is attributable to the Berachain-specific gate.
    fn upstream_valid_header(timestamp: u64, post_prague: bool) -> BerachainHeader {
        BerachainHeader {
            number: 1,
            timestamp,
            difficulty: U256::ZERO,
            nonce: Default::default(),
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            gas_limit: 30_000_000,
            gas_used: 0,
            base_fee_per_gas: Some(1_000_000_000),
            withdrawals_root: Some(EMPTY_WITHDRAWALS),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: Some(BlockHash::ZERO),
            requests_hash: post_prague.then_some(BlockHash::ZERO),
            ..Default::default()
        }
    }

    #[test]
    fn test_validate_header_rejects_pre_prague1_proposer_pubkey() {
        let chain_spec = bepolia_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());

        let mut header = upstream_valid_header(0, false);
        header.prev_proposer_pubkey = Some(mock_bls_pubkey());

        assert!(!chain_spec.is_prague1_active_at_timestamp(header.timestamp));

        let sealed = SealedHeader::new(header, BlockHash::ZERO);
        let err = consensus.validate_header(&sealed).expect_err(
            "pre-Prague1 header with prev_proposer_pubkey must be rejected by validate_header",
        );
        assert!(
            err.to_string().contains("not allowed"),
            "expected ProposerPubkeyNotAllowed, got: {err}"
        );
    }

    #[test]
    fn test_validate_header_rejects_post_prague1_missing_proposer_pubkey() {
        let chain_spec = bepolia_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());

        // Bepolia activates Prague1 at 1_754_496_000; pick a timestamp safely past it.
        let timestamp = 1_754_496_000;
        let mut header = upstream_valid_header(timestamp, true);
        header.prev_proposer_pubkey = None;

        assert!(chain_spec.is_prague1_active_at_timestamp(header.timestamp));

        let sealed = SealedHeader::new(header, BlockHash::ZERO);
        let err = consensus.validate_header(&sealed).expect_err(
            "post-Prague1 header missing prev_proposer_pubkey must be rejected by validate_header",
        );
        assert!(
            err.to_string().contains("Previous proposer public key is required"),
            "expected MissingProposerPubkey, got: {err}"
        );
    }

    #[test]
    fn test_validate_header_accepts_pre_prague1_no_proposer_pubkey() {
        let chain_spec = bepolia_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());

        let header = upstream_valid_header(0, false);
        assert!(header.prev_proposer_pubkey.is_none());

        let sealed = SealedHeader::new(header, BlockHash::ZERO);
        consensus
            .validate_header(&sealed)
            .expect("pre-Prague1 header without prev_proposer_pubkey should validate");
    }

    #[test]
    fn test_validate_header_accepts_post_prague1_proposer_pubkey() {
        let chain_spec = bepolia_chainspec();
        let consensus = BerachainBeaconConsensus::new(chain_spec.clone());

        let timestamp = 1_754_496_000;
        let mut header = upstream_valid_header(timestamp, true);
        header.prev_proposer_pubkey = Some(mock_bls_pubkey());

        assert!(chain_spec.is_prague1_active_at_timestamp(header.timestamp));

        let sealed = SealedHeader::new(header, BlockHash::ZERO);
        consensus
            .validate_header(&sealed)
            .expect("post-Prague1 header with prev_proposer_pubkey should validate");
    }
}
