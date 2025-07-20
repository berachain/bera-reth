use crate::{
    chainspec::BerachainChainSpec,
    engine::validate_proposer_pubkey_prague1,
    hardforks::BerachainHardforks,
    node::evm::{block_context::BerachainBlockExecutionCtx, error::BerachainExecutionError},
    primitives::{BerachainBlock, BerachainHeader},
    transaction::{BerachainTxEnvelope, BerachainTxType, pol::create_pol_transaction},
};
use alloy_consensus::{
    Block, BlockBody, BlockHeader, EMPTY_OMMER_ROOT_HASH, Transaction, TxReceipt, proofs,
};
use alloy_eips::merge::BEACON_NONCE;
use alloy_primitives::{Bytes, logs_bloom};
use reth::{chainspec::EthereumHardforks, providers::BlockExecutionResult};
use reth_chainspec::EthChainSpec;
use reth_ethereum_primitives::Receipt;
use reth_evm::{
    block::{BlockExecutionError, BlockExecutorFactory},
    execute::{BlockAssembler, BlockAssemblerInput},
};
use std::sync::Arc;
use tracing::info;

#[derive(Clone, Debug)]
pub struct BerachainBlockAssembler {
    /// The chainspec.
    pub chain_spec: Arc<BerachainChainSpec>,
    /// Extra data to use for the blocks.
    pub extra_data: Bytes,
}

impl BerachainBlockAssembler {
    /// Creates a new [`BerachainBlockAssembler`].
    pub fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { chain_spec, extra_data: Default::default() }
    }
}

impl<F> BlockAssembler<F> for BerachainBlockAssembler
where
    F: for<'a> BlockExecutorFactory<
            ExecutionCtx<'a> = BerachainBlockExecutionCtx<'a>,
            Transaction = BerachainTxEnvelope,
            Receipt = Receipt<BerachainTxType>,
        >,
{
    type Block = BerachainBlock;

    fn assemble_block(
        &self,
        input: BlockAssemblerInput<'_, '_, F, BerachainHeader>,
    ) -> Result<Self::Block, BlockExecutionError> {
        let BlockAssemblerInput {
            evm_env,
            execution_ctx: ctx,
            parent,
            mut transactions,
            output: BlockExecutionResult { receipts, requests, gas_used },
            state_root,
            ..
        } = input;

        info!(target: "block receipts", ?receipts, "block assembler");

        let timestamp = evm_env.block_env.timestamp.saturating_to();

        // Validate proposer pubkey presence for Prague1
        validate_proposer_pubkey_prague1(&*self.chain_spec, timestamp, ctx.prev_proposer_pubkey)?;

        // Check if Prague1 is active and we need to inject POL transaction
        if self.chain_spec.is_prague1_active_at_timestamp(timestamp) {
            let prev_proposer_pubkey = ctx.prev_proposer_pubkey.unwrap();

            // Synthesize POL transaction and prepend to transactions list
            let base_fee = evm_env.block_env.basefee;
            let pol_transaction = create_pol_transaction(
                self.chain_spec.clone(),
                prev_proposer_pubkey,
                evm_env.block_env.number,
                base_fee,
            )?;

            transactions.insert(0, pol_transaction);
            info!(target: "block assembler", "Injected POL transaction into block transaction list");

            // Validate that we have receipts after POL transaction execution
            if receipts.is_empty() {
                return Err(BerachainExecutionError::MissingPolReceipts.into());
            }

            // Validate that the first transaction in the list is indeed a POL transaction
            if let Some(first_tx) = transactions.first() {
                if !matches!(first_tx, BerachainTxEnvelope::Berachain(_)) {
                    return Err(BerachainExecutionError::MissingPolTransactionAtIndex0.into());
                }
            } else {
                return Err(BerachainExecutionError::MissingPolTransactionAtIndex0.into());
            }
        }

        let transactions_root = proofs::calculate_transaction_root(&transactions);
        let receipts_root = Receipt::calculate_receipt_root_no_memo(receipts);
        let logs_bloom = logs_bloom(receipts.iter().flat_map(|r| r.logs()));

        let withdrawals = self
            .chain_spec
            .is_shanghai_active_at_timestamp(timestamp)
            .then(|| ctx.withdrawals.map(|w| w.into_owned()).unwrap_or_default());

        let withdrawals_root =
            withdrawals.as_deref().map(|w| proofs::calculate_withdrawals_root(w));
        let requests_hash = self
            .chain_spec
            .is_prague_active_at_timestamp(timestamp)
            .then(|| requests.requests_hash());

        let mut excess_blob_gas = None;
        let mut blob_gas_used = None;

        // only determine cancun fields when active
        if self.chain_spec.is_cancun_active_at_timestamp(timestamp) {
            blob_gas_used =
                Some(transactions.iter().map(|tx| tx.blob_gas_used().unwrap_or_default()).sum());
            excess_blob_gas = if self.chain_spec.is_cancun_active_at_timestamp(parent.timestamp) {
                parent.maybe_next_block_excess_blob_gas(
                    self.chain_spec.blob_params_at_timestamp(timestamp),
                )
            } else {
                // for the first post-fork block, both parent.blob_gas_used and
                // parent.excess_blob_gas are evaluated as 0
                Some(alloy_eips::eip7840::BlobParams::cancun().next_block_excess_blob_gas(0, 0))
            };
        }

        let header = BerachainHeader {
            parent_hash: ctx.parent_hash,
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            beneficiary: evm_env.block_env.beneficiary,
            state_root,
            transactions_root,
            receipts_root,
            withdrawals_root,
            logs_bloom,
            timestamp,
            mix_hash: evm_env.block_env.prevrandao.unwrap_or_default(),
            nonce: BEACON_NONCE.into(),
            base_fee_per_gas: Some(evm_env.block_env.basefee),
            number: evm_env.block_env.number.saturating_to(),
            gas_limit: evm_env.block_env.gas_limit,
            difficulty: evm_env.block_env.difficulty,
            gas_used: *gas_used,
            extra_data: self.extra_data.clone(),
            parent_beacon_block_root: ctx.parent_beacon_block_root,
            blob_gas_used,
            excess_blob_gas,
            requests_hash,
            prev_proposer_pubkey: ctx.prev_proposer_pubkey,
        };

        Ok(Block {
            header,
            body: BlockBody { transactions, ommers: Default::default(), withdrawals },
        })
    }
}
