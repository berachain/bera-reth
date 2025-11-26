use crate::{
    chainspec::BerachainChainSpec,
    engine::{
        BerachainEngineTypes,
        payload::{BerachainBuiltPayload, BerachainPayloadBuilderAttributes},
    },
    hardforks::BerachainHardforks,
    node::{
        BerachainNode,
        evm::config::{BerachainEvmConfig, BerachainNextBlockEnvAttributes},
    },
    pool::transaction::BerachainPooledTransaction,
    primitives::BerachainHeader,
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::Transaction;
use alloy_eips::eip1559::ETHEREUM_BLOCK_GAS_LIMIT_30M;
use alloy_primitives::U256;
use alloy_rlp::Encodable;
use rblib::{
    alloy::evm::{
        Evm,
        revm::{context::Block, database::State},
    },
    prelude::*,
    reth::{
        errors::{BlockExecutionError, BlockValidationError, ConsensusError},
        ethereum::{
            chainspec::EthereumHardforks, evm::revm::database::StateProviderDatabase,
            provider::StateProvider,
        },
    },
};
use reth::api::PayloadTypes;
use reth_basic_payload_builder::{BuildArguments, BuildOutcome, PayloadConfig, is_better_payload};
use reth_chainspec::EthChainSpec;
use reth_consensus_common::validation::MAX_RLP_BLOCK_SIZE;
use reth_ethereum_engine_primitives::BlobSidecars;
use reth_ethereum_payload_builder::EthereumBuilderConfig;
use reth_evm::{
    ConfigureEvm,
    execute::{BlockBuilder, BlockBuilderOutcome},
};
use reth_payload_primitives::PayloadBuilderAttributes;
use reth_primitives_traits::transaction::error::InvalidTransactionError;
use reth_transaction_pool::{
    BestTransactions, BestTransactionsAttributes, BestTransactionsFor, PoolTransaction,
    TransactionPool, ValidPoolTransaction,
    error::{Eip4844PoolTransactionError, InvalidPoolTransactionError},
    noop::NoopTransactionPool,
};
use std::sync::Arc;
use tracing::{debug, trace, warn};

use crate::evm::BerachainEvmFactory;
use rblib::prelude::pool::FixedTransactions;
use std::time::Duration;

/// Default limits for Berachain payload building
#[derive(Debug, Clone, Default)]
pub struct BerachainLimits;

impl PlatformLimits<BerachainPlatform> for BerachainLimits {
    fn create(&self, _block: &BlockContext<BerachainPlatform>) -> Limits {
        Limits {
            gas_limit: 30_000_000, // TODO: calculate correctly
            blob_params: None,     // TODO: Get blob configuration from chainspec
            max_transactions: Some(1_000),
            deadline: Some(Duration::from_secs(2)), // bera block time
        }
    }
}

/// Platform implementation for Berachain
///
/// This type implements rblib's Platform trait for Berachain,
/// allowing the use of rblib's advanced block building capabilities.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BerachainPlatform;

impl Platform for BerachainPlatform {
    type NodeTypes = BerachainNode;
    type EvmConfig = BerachainEvmConfig;
    type PooledTransaction = BerachainPooledTransaction;
    type Bundle = FlashbotsBundle<Self>;
    type DefaultLimits = BerachainLimits;

    fn evm_config<P>(chainspec: Arc<BerachainChainSpec>) -> BerachainEvmConfig
    where
        P: traits::PlatformExecBounds<Self>,
    {
        BerachainEvmConfig::new_with_evm_factory(chainspec, BerachainEvmFactory::default())
    }

    fn next_block_environment_context<P>(
        _chainspec: &BerachainChainSpec,
        _parent: &BerachainHeader,
        attributes: &BerachainPayloadBuilderAttributes,
    ) -> <BerachainEvmConfig as reth::api::ConfigureEvm>::NextBlockEnvCtx
    where
        P: traits::PlatformExecBounds<Self>,
    {
        use crate::node::evm::config::BerachainNextBlockEnvAttributes;

        BerachainNextBlockEnvAttributes {
            timestamp: attributes.timestamp,
            suggested_fee_recipient: attributes.suggested_fee_recipient,
            prev_randao: attributes.prev_randao,
            gas_limit: ETHEREUM_BLOCK_GAS_LIMIT_30M, // TODO: Get from config
            parent_beacon_block_root: attributes.parent_beacon_block_root,
            withdrawals: Some(attributes.withdrawals.clone()),
            prev_proposer_pubkey: attributes.prev_proposer_pubkey,
        }
    }

    fn build_payload<P>(
        payload: Checkpoint<P>,
        provider: &dyn StateProvider,
    ) -> Result<<BerachainEngineTypes as PayloadTypes>::BuiltPayload, PayloadBuilderError>
    where
        P: traits::PlatformExecBounds<Self>,
    {
        let evm_config = payload.block().evm_config().clone();
        let chain_spec = payload.block().chainspec();

        let payload_config = PayloadConfig::new(
            Arc::new(payload.block().parent().clone()),
            payload.block().attributes().clone(),
        );

        let build_args =
            BuildArguments::new(Default::default(), payload_config, Default::default(), None);

        let builder_config = EthereumBuilderConfig::new();

        // This will reorder transactions. Something we want to avoid for flashblocks.
        let transactions = payload.history().transactions().cloned().collect();
        let transactions = Box::new(FixedTransactions::<Self>::new(transactions));

        default_berachain_payload_for_platform(
            evm_config,
            chain_spec,
            provider,
            NoopTransactionPool::new(),
            &builder_config,
            build_args,
            |_| {
                transactions
                    as Box<
                        dyn BestTransactions<
                            Item = Arc<ValidPoolTransaction<Self::PooledTransaction>>,
                        >,
                    >
            },
        )?
        .into_payload()
        .ok_or_else(|| PayloadBuilderError::MissingPayload)
    }
}

#[inline]
pub fn default_berachain_payload_for_platform<Pool, F>(
    evm_config: BerachainEvmConfig,
    chain_spec: &Arc<types::ChainSpec<BerachainPlatform>>,
    state_provider: &dyn StateProvider,
    pool: Pool,
    builder_config: &EthereumBuilderConfig,
    args: BuildArguments<BerachainPayloadBuilderAttributes, BerachainBuiltPayload>,
    best_txs: F,
) -> Result<BuildOutcome<BerachainBuiltPayload>, PayloadBuilderError>
where
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = BerachainTxEnvelope>>,
    F: FnOnce(BestTransactionsAttributes) -> BestTransactionsFor<Pool>,
{
    let BuildArguments { mut cached_reads, config, cancel, best_payload } = args;
    let PayloadConfig { parent_header, attributes } = config;

    let state = StateProviderDatabase::new(&state_provider);
    let mut db =
        State::builder().with_database(cached_reads.as_db_mut(state)).with_bundle_update().build();

    let mut builder = evm_config
        .builder_for_next_block(
            &mut db,
            &parent_header,
            BerachainNextBlockEnvAttributes {
                timestamp: attributes.timestamp(),
                suggested_fee_recipient: attributes.suggested_fee_recipient(),
                prev_randao: attributes.prev_randao(),
                gas_limit: builder_config.gas_limit(parent_header.gas_limit),
                parent_beacon_block_root: attributes.parent_beacon_block_root(),
                withdrawals: Some(attributes.withdrawals().clone()),
                prev_proposer_pubkey: attributes.prev_proposer_pubkey,
            },
        )
        .map_err(PayloadBuilderError::other)?;

    debug!(target: "payload_builder", id=%attributes.id, parent_header = ?parent_header.hash(), parent_number = parent_header.number, "building new payload");
    let mut cumulative_gas_used = 0;
    let block_gas_limit: u64 = builder.evm_mut().block().gas_limit;
    let base_fee = builder.evm_mut().block().basefee;

    let mut best_txs = best_txs(BestTransactionsAttributes::new(
        base_fee,
        builder.evm_mut().block().blob_gasprice().map(|gasprice| gasprice as u64),
    ));
    let mut total_fees = U256::ZERO;

    builder.apply_pre_execution_changes().map_err(|err| {
        warn!(target: "payload_builder", %err, "failed to apply pre-execution changes");
        PayloadBuilderError::Internal(err.into())
    })?;

    // initialize empty blob sidecars at first. If cancun is active then this will be populated by
    // blob sidecars if any.
    let mut blob_sidecars = BlobSidecars::Empty;

    let mut block_blob_count = 0;
    let mut block_transactions_rlp_length = 0;

    let blob_params = chain_spec.blob_params_at_timestamp(attributes.timestamp);
    let max_blob_count =
        blob_params.as_ref().map(|params| params.max_blob_count).unwrap_or_default();

    let is_osaka = chain_spec.is_osaka_active_at_timestamp(attributes.timestamp);

    // Check if Prague3 is active and skip all transactions if so
    if chain_spec.is_prague3_active_at_timestamp(attributes.timestamp()) {
        warn!(target: "payload_builder", "Prague3 is active, building payload without transactions is not supported");
        return Err(PayloadBuilderError::Other(Box::from(
            "Prague 3 block building is not supported",
        )))
    }
    // Skip all transactions and proceed to finalize the empty block
    while let Some(pool_tx) = best_txs.next() {
        // ensure we still have capacity for this transaction
        if cumulative_gas_used + pool_tx.gas_limit() > block_gas_limit {
            // we can't fit this transaction into the block, so we need to mark it as invalid
            // which also removes all dependent transaction from the iterator before we can
            // continue
            best_txs.mark_invalid(
                &pool_tx,
                InvalidPoolTransactionError::ExceedsGasLimit(pool_tx.gas_limit(), block_gas_limit),
            );
            continue
        }

        // check if the job was cancelled, if so we can exit early
        if cancel.is_cancelled() {
            return Ok(BuildOutcome::Cancelled)
        }

        // convert tx to a signed transaction
        let tx = pool_tx.to_consensus();

        let estimated_block_size_with_tx = block_transactions_rlp_length +
            tx.inner().length() +
            attributes.withdrawals().length() +
            1024; // 1Kb of overhead for the block header

        if is_osaka && estimated_block_size_with_tx > MAX_RLP_BLOCK_SIZE {
            best_txs.mark_invalid(
                &pool_tx,
                InvalidPoolTransactionError::OversizedData(
                    estimated_block_size_with_tx,
                    MAX_RLP_BLOCK_SIZE,
                ),
            );
            continue;
        }

        // There's only limited amount of blob space available per block, so we need to check if
        // the EIP-4844 can still fit in the block
        let mut blob_tx_sidecar = None;
        if let Some(blob_tx) = tx.as_eip4844() {
            let tx_blob_count = blob_tx.tx().blob_versioned_hashes.len() as u64;

            if block_blob_count + tx_blob_count > max_blob_count {
                // we can't fit this _blob_ transaction into the block, so we mark it as
                // invalid, which removes its dependent transactions from
                // the iterator. This is similar to the gas limit condition
                // for regular transactions above.
                trace!(target: "payload_builder", tx=?tx.hash(), ?block_blob_count, "skipping blob transaction because it would exceed the max blob count per block");
                best_txs.mark_invalid(
                    &pool_tx,
                    InvalidPoolTransactionError::Eip4844(
                        Eip4844PoolTransactionError::TooManyEip4844Blobs {
                            have: block_blob_count + tx_blob_count,
                            permitted: max_blob_count,
                        },
                    ),
                );
                continue
            }

            let blob_sidecar_result = 'sidecar: {
                let Some(sidecar) =
                    pool.get_blob(*tx.hash()).map_err(PayloadBuilderError::other)?
                else {
                    break 'sidecar Err(Eip4844PoolTransactionError::MissingEip4844BlobSidecar)
                };

                if is_osaka {
                    if sidecar.is_eip7594() {
                        Ok(sidecar)
                    } else {
                        Err(Eip4844PoolTransactionError::UnexpectedEip4844SidecarAfterOsaka)
                    }
                } else if sidecar.is_eip4844() {
                    Ok(sidecar)
                } else {
                    Err(Eip4844PoolTransactionError::UnexpectedEip7594SidecarBeforeOsaka)
                }
            };

            blob_tx_sidecar = match blob_sidecar_result {
                Ok(sidecar) => Some(sidecar),
                Err(error) => {
                    best_txs.mark_invalid(&pool_tx, InvalidPoolTransactionError::Eip4844(error));
                    continue
                }
            };
        }

        // Execute the transaction
        let gas_used = match builder.execute_transaction(tx.clone()) {
            Ok(gas_used) => gas_used,
            Err(BlockExecutionError::Validation(BlockValidationError::InvalidTx {
                error, ..
            })) => {
                if error.is_nonce_too_low() {
                    // if the nonce is too low, we can skip this transaction
                    trace!(target: "payload_builder", %error, ?tx, "skipping nonce too low transaction");
                } else {
                    // if the transaction is invalid, we can skip it and all of its
                    // descendants
                    trace!(target: "payload_builder", %error, ?tx, "skipping invalid transaction and its descendants");
                    best_txs.mark_invalid(
                        &pool_tx,
                        InvalidPoolTransactionError::Consensus(
                            InvalidTransactionError::TxTypeNotSupported,
                        ),
                    );
                }
                continue
            }
            // this is an error that we should treat as fatal for this attempt
            Err(err) => return Err(PayloadBuilderError::evm(err)),
        };

        // add to the total blob gas used if the transaction successfully executed
        if let Some(blob_tx) = tx.as_eip4844() {
            block_blob_count += blob_tx.tx().blob_versioned_hashes.len() as u64;

            // if we've reached the max blob count, we can skip blob txs entirely
            if block_blob_count == max_blob_count {
                best_txs.skip_blobs();
            }
        }

        block_transactions_rlp_length += tx.inner().length();

        // update and add to total fees
        let miner_fee =
            tx.effective_tip_per_gas(base_fee).expect("fee is always valid; execution succeeded");
        total_fees += U256::from(miner_fee) * U256::from(gas_used);
        cumulative_gas_used += gas_used;

        // Add blob tx sidecar to the payload.
        if let Some(sidecar) = blob_tx_sidecar {
            blob_sidecars.push_sidecar_variant(sidecar.as_ref().clone());
        }
    }

    // check if we have a better block
    if !is_better_payload(best_payload.as_ref(), total_fees) {
        // Release db
        drop(builder);
        // can skip building the block
        return Ok(BuildOutcome::Aborted { fees: total_fees, cached_reads })
    }

    let BlockBuilderOutcome { execution_result, block, .. } = builder.finish(state_provider)?;

    let requests = chain_spec
        .is_prague_active_at_timestamp(attributes.timestamp)
        .then_some(execution_result.requests);

    let sealed_block = Arc::new(block.sealed_block().clone());
    debug!(target: "payload_builder", id=%attributes.id, sealed_block_header = ?sealed_block.sealed_header(), "sealed built block");

    if is_osaka && sealed_block.rlp_length() > MAX_RLP_BLOCK_SIZE {
        return Err(PayloadBuilderError::other(ConsensusError::BlockTooLarge {
            rlp_length: sealed_block.rlp_length(),
            max_rlp_length: MAX_RLP_BLOCK_SIZE,
        }));
    }

    let payload = BerachainBuiltPayload::new(attributes.id, sealed_block, total_fees, requests)
        // add blob sidecars from the executed txs
        .with_sidecars(blob_sidecars);

    Ok(BuildOutcome::Better { payload, cached_reads })
}
