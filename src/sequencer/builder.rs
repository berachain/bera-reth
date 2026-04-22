//! Flashblock-aware payload builder for sequencer mode.
//!
//! This builder produces flashblocks at regular intervals (~200ms) while building
//! a block, publishing them via WebSocket for preconfirmation subscribers.

use crate::{
    chainspec::BerachainChainSpec,
    engine::payload::{BerachainBuiltPayload, BerachainPayloadBuilderAttributes},
    flashblocks::{
        BerachainFlashblockPayload, BerachainFlashblockPayloadBase, BerachainFlashblockPayloadDiff,
        BerachainFlashblockPayloadMetadata,
    },
    hardforks::BerachainHardforks,
    node::evm::config::{BerachainEvmConfig, BerachainNextBlockEnvAttributes},
    primitives::BerachainHeader,
    sequencer::{
        SequencerConfig, WebSocketPublisher,
        signing::{FlashblockSigner, compute_transactions_hash},
    },
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::Transaction;
use alloy_eips::{eip2718::Encodable2718, eip4895::Withdrawal};
use alloy_primitives::{Bytes, U256};
use reth::{
    api::{FullNodeTypes, NodeTypes, PayloadBuilderError, PayloadTypes, TxTy},
    chainspec::EthereumHardforks,
    providers::StateProviderFactory,
    revm::{State, context::Block, database::StateProviderDatabase},
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_basic_payload_builder::{
    BuildArguments, BuildOutcome, MissingPayloadBehaviour, PayloadBuilder, PayloadConfig,
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_ethereum_engine_primitives::BlobSidecars;
use reth_ethereum_payload_builder::EthereumBuilderConfig;
use reth_ethereum_primitives::Receipt;
use reth_evm::{
    ConfigureEvm, Evm,
    block::{BlockExecutionError, BlockValidationError, CommitChanges},
    execute::{BlockBuilder, BlockBuilderOutcome},
};
use reth_node_builder::{BuilderContext, PayloadBuilderConfig, components::PayloadBuilderBuilder};
use reth_payload_primitives::PayloadBuilderAttributes;
use reth_primitives_traits::transaction::error::InvalidTransactionError;
use reth_transaction_pool::{
    BestTransactions, BestTransactionsAttributes, ValidPoolTransaction,
    error::{Eip4844PoolTransactionError, InvalidPoolTransactionError},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{debug, info, trace, warn};

use crate::{
    sequencer::{
        FlashblockSequencerMetrics, record_build_exit, record_emitted, record_publish_error,
    },
    transaction::BerachainTxType,
};

type BestTransactionsIter<Pool> = Box<
    dyn BestTransactions<Item = Arc<ValidPoolTransaction<<Pool as TransactionPool>::Transaction>>>,
>;

/// Service builder for creating flashblock-aware payload builders.
#[derive(Clone, Debug)]
pub struct FlashblockPayloadServiceBuilder {
    config: SequencerConfig,
    publisher: Arc<WebSocketPublisher>,
}

impl FlashblockPayloadServiceBuilder {
    /// Create a new flashblock payload service builder.
    pub fn new(config: SequencerConfig, publisher: Arc<WebSocketPublisher>) -> Self {
        Self { config, publisher }
    }
}

impl<Types, Node, Pool> PayloadBuilderBuilder<Node, Pool, BerachainEvmConfig>
    for FlashblockPayloadServiceBuilder
where
    Types: NodeTypes<
            ChainSpec = BerachainChainSpec,
            Primitives = crate::primitives::BerachainPrimitives,
        >,
    Node: FullNodeTypes<Types = Types>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TxTy<Node::Types>>>
        + Unpin
        + 'static,
    Types::Payload: PayloadTypes<
            BuiltPayload = BerachainBuiltPayload,
            PayloadAttributes = crate::engine::payload::BerachainPayloadAttributes,
            PayloadBuilderAttributes = BerachainPayloadBuilderAttributes,
        >,
{
    type PayloadBuilder = FlashblockPayloadBuilder<Pool, Node::Provider>;

    async fn build_payload_builder(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
        evm_config: BerachainEvmConfig,
    ) -> eyre::Result<Self::PayloadBuilder> {
        let conf = ctx.payload_builder_config();
        let chain = ctx.chain_spec().chain();
        let gas_limit = conf.gas_limit_for(chain);

        Ok(FlashblockPayloadBuilder::new(
            ctx.provider().clone(),
            pool,
            evm_config,
            EthereumBuilderConfig::new().with_gas_limit(gas_limit),
            self.config,
            self.publisher,
            conf.deadline(),
        ))
    }
}

/// Flashblock-aware payload builder.
///
/// This builder emits flashblocks at regular intervals while building a payload,
/// allowing preconfirmation subscribers to track transaction inclusion in real-time.
#[derive(Debug)]
pub struct FlashblockPayloadBuilder<Pool, Client> {
    client: Client,
    pool: Pool,
    evm_config: BerachainEvmConfig,
    builder_config: EthereumBuilderConfig,
    sequencer_config: SequencerConfig,
    publisher: Arc<WebSocketPublisher>,
    deadline: Duration,
    payload_requested: Arc<AtomicBool>,
}

impl<Pool: Clone, Client: Clone> Clone for FlashblockPayloadBuilder<Pool, Client> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            pool: self.pool.clone(),
            evm_config: self.evm_config.clone(),
            builder_config: self.builder_config.clone(),
            sequencer_config: self.sequencer_config.clone(),
            publisher: self.publisher.clone(),
            deadline: self.deadline,
            payload_requested: self.payload_requested.clone(),
        }
    }
}

impl<Pool, Client> FlashblockPayloadBuilder<Pool, Client> {
    /// Create a new flashblock payload builder.
    pub fn new(
        client: Client,
        pool: Pool,
        evm_config: BerachainEvmConfig,
        builder_config: EthereumBuilderConfig,
        sequencer_config: SequencerConfig,
        publisher: Arc<WebSocketPublisher>,
        deadline: Duration,
    ) -> Self {
        Self {
            client,
            pool,
            evm_config,
            builder_config,
            sequencer_config,
            publisher,
            deadline,
            payload_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl<Pool, Client> PayloadBuilder for FlashblockPayloadBuilder<Pool, Client>
where
    Client: StateProviderFactory + ChainSpecProvider<ChainSpec = BerachainChainSpec> + Clone,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = BerachainTxEnvelope>>,
{
    type Attributes = BerachainPayloadBuilderAttributes;
    type BuiltPayload = BerachainBuiltPayload;

    fn try_build(
        &self,
        args: BuildArguments<Self::Attributes, BerachainBuiltPayload>,
    ) -> Result<BuildOutcome<BerachainBuiltPayload>, PayloadBuilderError> {
        self.payload_requested.store(false, Ordering::Relaxed);
        build_flashblock_payload(
            self.evm_config.clone(),
            self.client.clone(),
            self.pool.clone(),
            self.builder_config.clone(),
            self.sequencer_config.clone(),
            self.publisher.clone(),
            self.deadline,
            self.payload_requested.clone(),
            args,
            |attributes| self.pool.best_transactions_with_attributes(attributes),
        )
    }

    fn on_missing_payload(
        &self,
        _args: BuildArguments<Self::Attributes, Self::BuiltPayload>,
    ) -> MissingPayloadBehaviour<Self::BuiltPayload> {
        self.payload_requested.store(true, Ordering::Relaxed);
        MissingPayloadBehaviour::AwaitInProgress
    }

    fn build_empty_payload(
        &self,
        config: PayloadConfig<BerachainPayloadBuilderAttributes, BerachainHeader>,
    ) -> Result<BerachainBuiltPayload, PayloadBuilderError> {
        warn!(target: "sequencer::builder", "build_empty_payload called, no payload was ready in time");
        let args = BuildArguments::new(Default::default(), config, Default::default(), None);
        self.try_build(args)?.into_payload().ok_or_else(|| PayloadBuilderError::MissingPayload)
    }
}

/// Tracks execution data for flashblock emission.
struct FlashblockExecutionTracker {
    /// Cumulative receipts for all executed transactions.
    receipts: Vec<Receipt<BerachainTxType>>,
    /// Encoded transactions for the current interval.
    interval_transactions: Vec<Bytes>,
    /// All encoded transactions.
    all_transactions: Vec<Bytes>,
    /// Cumulative gas used.
    cumulative_gas_used: u64,
    /// Total fees collected.
    total_fees: U256,
}

impl FlashblockExecutionTracker {
    fn new() -> Self {
        Self {
            receipts: Vec::new(),
            interval_transactions: Vec::new(),
            all_transactions: Vec::new(),
            cumulative_gas_used: 0,
            total_fees: U256::ZERO,
        }
    }

    /// Clear interval transactions for next flashblock.
    fn clear_interval(&mut self) {
        self.interval_transactions.clear();
    }
}

/// Build a payload while emitting flashblocks at regular intervals.
#[allow(clippy::too_many_arguments)]
fn build_flashblock_payload<Client, Pool, F>(
    evm_config: BerachainEvmConfig,
    client: Client,
    pool: Pool,
    builder_config: EthereumBuilderConfig,
    sequencer_config: SequencerConfig,
    publisher: Arc<WebSocketPublisher>,
    deadline: Duration,
    payload_requested: Arc<AtomicBool>,
    args: BuildArguments<BerachainPayloadBuilderAttributes, BerachainBuiltPayload>,
    best_txs: F,
) -> Result<BuildOutcome<BerachainBuiltPayload>, PayloadBuilderError>
where
    Client: StateProviderFactory + ChainSpecProvider<ChainSpec = BerachainChainSpec>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = BerachainTxEnvelope>>,
    F: FnOnce(BestTransactionsAttributes) -> BestTransactionsIter<Pool>,
{
    let BuildArguments { mut cached_reads, config, cancel: _, best_payload: _ } = args;
    let PayloadConfig { parent_header, attributes } = config;

    let state_provider = client.state_by_block_hash(parent_header.hash())?;
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
                extra_data: Default::default(),
            },
        )
        .map_err(PayloadBuilderError::other)?;

    let chain_spec = client.chain_spec();
    let payload_id = attributes.id;
    let block_number = parent_header.number + 1;

    info!(
        target: "sequencer::builder",
        id = %payload_id,
        parent_hash = ?parent_header.hash(),
        parent_number = parent_header.number,
        block_number,
        timestamp = attributes.timestamp(),
        deadline_ms = deadline.as_millis(),
        interval_ms = sequencer_config.interval.as_millis(),
        "starting flashblock payload build"
    );

    let block_gas_limit: u64 = builder.evm_mut().block().gas_limit;
    let base_fee = builder.evm_mut().block().basefee;

    let mut best_txs = best_txs(BestTransactionsAttributes::new(
        base_fee,
        builder.evm_mut().block().blob_gasprice().map(|gasprice| gasprice as u64),
    ));

    // Apply pre-execution changes (PoL, withdrawals, etc.)
    builder.apply_pre_execution_changes().map_err(|err| {
        record_build_exit("pre_exec_failed");
        warn!(target: "sequencer::builder", %err, "failed to apply pre-execution changes");
        PayloadBuilderError::Internal(err.into())
    })?;

    // Check if Prague3 is active
    if chain_spec.is_prague3_active_at_timestamp(attributes.timestamp()) {
        record_build_exit("prague3_rejected");
        return Err(PayloadBuilderError::Other(Box::from(
            "Prague 3 block building is not supported",
        )));
    }

    // Build the base payload for flashblock 0
    let base = BerachainFlashblockPayloadBase {
        parent_beacon_block_root: attributes.parent_beacon_block_root().unwrap_or_default(),
        parent_hash: parent_header.hash(),
        fee_recipient: attributes.suggested_fee_recipient(),
        prev_randao: attributes.prev_randao(),
        block_number,
        gas_limit: block_gas_limit,
        timestamp: attributes.timestamp(),
        extra_data: Bytes::default(),
        base_fee_per_gas: U256::from(base_fee),
        prev_proposer_pubkey: attributes.prev_proposer_pubkey,
    };

    // Withdrawals go in first flashblock (index 0)
    let withdrawals: Vec<Withdrawal> = attributes.withdrawals().to_vec();

    let mut tracker = FlashblockExecutionTracker::new();
    let mut blob_sidecars = BlobSidecars::Empty;
    let mut flashblock_index = 0u64;
    let mut last_emitted_cumulative_gas: u64 = 0;
    let mut last_flashblock_time = Instant::now();
    let interval = sequencer_config.interval;
    let build_start_time = Instant::now();
    let metrics = FlashblockSequencerMetrics::default();

    // Helper to emit flashblock. Per BRIP-0007, no state root computation needed.
    let emit = |flashblock_index: u64,
                tracker: &FlashblockExecutionTracker,
                interval_gas: u64,
                is_last: bool| {
        emit_flashblock(
            &publisher,
            payload_id,
            flashblock_index,
            &base,
            flashblock_index == 0,
            tracker,
            interval_gas,
            block_number,
            &sequencer_config.signer,
            &withdrawals,
            is_last,
            &metrics,
        );
    };

    // Main transaction execution loop with flashblock emission.
    // Flashblocks are emitted at regular intervals (~200ms) regardless of transaction activity.
    // Empty flashblocks serve as heartbeats, allowing subscribers to detect liveness and
    // track the current state even when no transactions are being processed.
    loop {
        if payload_requested.load(Ordering::Relaxed) {
            record_build_exit("payload_requested");
            info!(
                target: "sequencer::builder",
                id = %payload_id,
                flashblock_index,
                total_txs = tracker.all_transactions.len(),
                cumulative_gas = tracker.cumulative_gas_used,
                "payload build cancelled (getPayload called), finalizing"
            );
            break;
        }

        // Check if deadline exceeded (--builder.deadline).
        if build_start_time.elapsed() >= deadline {
            record_build_exit("deadline");
            info!(
                target: "sequencer::builder",
                id = %payload_id,
                flashblock_index,
                total_txs = tracker.all_transactions.len(),
                cumulative_gas = tracker.cumulative_gas_used,
                deadline_ms = deadline.as_millis(),
                "payload build deadline reached, finalizing"
            );
            break;
        }

        // Check if gas limit reached (use a small buffer to account for minimum tx gas)
        if tracker.cumulative_gas_used + 21_000 > block_gas_limit {
            record_build_exit("gas_limit");
            info!(
                target: "sequencer::builder",
                id = %payload_id,
                flashblock_index,
                total_txs = tracker.all_transactions.len(),
                cumulative_gas = tracker.cumulative_gas_used,
                block_gas_limit,
                "block gas limit reached, finalizing"
            );
            break;
        }

        // Emit flashblock at regular intervals (may be empty, serving as heartbeat)
        if last_flashblock_time.elapsed() >= interval {
            let actual = last_flashblock_time.elapsed();
            let drift = actual.saturating_sub(interval);
            let interval_gas =
                tracker.cumulative_gas_used.saturating_sub(last_emitted_cumulative_gas);
            metrics.interval_drift_seconds.record(drift.as_secs_f64());
            emit(flashblock_index, &tracker, interval_gas, false);
            flashblock_index += 1;
            last_emitted_cumulative_gas = tracker.cumulative_gas_used;
            tracker.clear_interval();
            last_flashblock_time = Instant::now();
        }

        // Try to get the next transaction
        let Some(pool_tx) = best_txs.next() else {
            // No transactions available, sleep briefly to prevent busy-waiting and check again
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };

        // Check gas limit
        if tracker.cumulative_gas_used + pool_tx.gas_limit() > block_gas_limit {
            best_txs.mark_invalid(
                &pool_tx,
                &InvalidPoolTransactionError::ExceedsGasLimit(pool_tx.gas_limit(), block_gas_limit),
            );
            continue;
        }

        let tx = pool_tx.to_consensus();
        let tx_hash = *tx.hash();

        // Fetch blob sidecar before execution so we can skip the tx if it's missing
        let mut blob_tx_sidecar = None;
        if tx.as_eip4844().is_some() {
            match pool.get_blob(tx_hash).map_err(PayloadBuilderError::other)? {
                Some(sidecar) => {
                    blob_tx_sidecar = Some(sidecar);
                }
                None => {
                    best_txs.mark_invalid(
                        &pool_tx,
                        &InvalidPoolTransactionError::Eip4844(
                            Eip4844PoolTransactionError::MissingEip4844BlobSidecar,
                        ),
                    );
                    continue;
                }
            }
        }

        // Execute the transaction and capture the result
        let mut execution_logs = Vec::new();
        let mut tx_success = false;

        let result = builder.execute_transaction_with_commit_condition(tx.clone(), |exec_result| {
            tx_success = exec_result.is_success();
            if tx_success {
                execution_logs = exec_result.logs().to_vec();
            }
            CommitChanges::Yes
        });

        let gas_used = match result {
            Ok(Some(gas)) => gas,
            Ok(None) => continue, // Transaction was not committed
            Err(BlockExecutionError::Validation(BlockValidationError::InvalidTx {
                error, ..
            })) => {
                if error.is_nonce_too_low() {
                    trace!(target: "sequencer::builder", %error, ?tx, "skipping nonce too low transaction");
                } else {
                    trace!(target: "sequencer::builder", %error, ?tx, "skipping invalid transaction");
                    best_txs.mark_invalid(
                        &pool_tx,
                        &InvalidPoolTransactionError::Consensus(
                            InvalidTransactionError::TxTypeNotSupported,
                        ),
                    );
                }
                continue;
            }
            Err(err) => return Err(PayloadBuilderError::evm(err)),
        };

        // Build receipt from execution result
        let receipt = Receipt {
            tx_type: tx.tx_type(),
            success: tx_success,
            cumulative_gas_used: tracker.cumulative_gas_used + gas_used,
            logs: execution_logs,
        };
        tracker.receipts.push(receipt);

        if let Some(sidecar) = blob_tx_sidecar {
            blob_sidecars.push_sidecar_variant(sidecar.as_ref().clone());
        }

        // Encode transaction
        let tx_bytes = Bytes::from(tx.inner().encoded_2718());

        // Update tracking
        let miner_fee =
            tx.effective_tip_per_gas(base_fee).expect("fee is always valid; execution succeeded");
        tracker.total_fees += U256::from(miner_fee) * U256::from(gas_used);
        tracker.cumulative_gas_used += gas_used;

        tracker.all_transactions.push(tx_bytes.clone());
        tracker.interval_transactions.push(tx_bytes);

        trace!(
            target: "sequencer::builder",
            tx_hash = ?tx_hash,
            gas_used,
            cumulative_gas_used = tracker.cumulative_gas_used,
            "executed transaction"
        );
    }

    // Always emit the final flashblock marked as last, even if empty.
    // This signals to RPC nodes that no more flashblocks will arrive for this payload.
    let final_interval_gas =
        tracker.cumulative_gas_used.saturating_sub(last_emitted_cumulative_gas);
    emit(flashblock_index, &tracker, final_interval_gas, true);

    // Finalize the block
    let BlockBuilderOutcome { execution_result, block, .. } = builder.finish(&state_provider)?;

    let requests = chain_spec
        .is_prague_active_at_timestamp(attributes.timestamp())
        .then_some(execution_result.requests);

    metrics.build_duration_seconds.record(build_start_time.elapsed().as_secs_f64());

    let sealed_block = Arc::new(block.sealed_block().clone());
    info!(
        target: "sequencer::builder",
        id = %payload_id,
        block_hash = %sealed_block.hash(),
        block_number = sealed_block.number,
        total_transactions = tracker.all_transactions.len(),
        total_fees = %tracker.total_fees,
        "sealed flashblock payload ready for getPayload"
    );

    let payload =
        BerachainBuiltPayload::new(payload_id, sealed_block, tracker.total_fees, requests)
            .with_sidecars(blob_sidecars);

    Ok(BuildOutcome::Better { payload, cached_reads })
}

#[allow(clippy::too_many_arguments)]
fn emit_flashblock(
    publisher: &WebSocketPublisher,
    payload_id: reth::rpc::types::engine::PayloadId,
    index: u64,
    base: &BerachainFlashblockPayloadBase,
    include_base_in_payload: bool,
    tracker: &FlashblockExecutionTracker,
    interval_gas: u64,
    block_number: u64,
    signer: &FlashblockSigner,
    withdrawals: &[Withdrawal],
    is_last: bool,
    metrics: &FlashblockSequencerMetrics,
) {
    metrics.gas_used_per_flashblock.record(interval_gas as f64);

    // Withdrawals are included in first flashblock only (index 0)
    let diff_withdrawals = if include_base_in_payload { withdrawals.to_vec() } else { vec![] };

    // Flashblocks just contain transactions, no computed roots
    let diff = BerachainFlashblockPayloadDiff {
        transactions: tracker.interval_transactions.clone(),
        withdrawals: diff_withdrawals,
    };

    // Sign over transactions hash
    let tx_hash = compute_transactions_hash(&tracker.interval_transactions);
    let sign_start = Instant::now();
    let signature = signer.sign_flashblock(block_number, payload_id, index, tx_hash);
    metrics.signing_duration_seconds.record(sign_start.elapsed().as_secs_f64());

    let flashblock = BerachainFlashblockPayload {
        payload_id,
        index,
        base: if include_base_in_payload { Some(base.clone()) } else { None },
        diff,
        metadata: BerachainFlashblockPayloadMetadata { block_number },
        signature,
        is_last,
    };

    record_emitted(is_last);
    metrics.transactions_per_flashblock.record(tracker.interval_transactions.len() as f64);

    match publisher.publish(&flashblock) {
        Ok((count, bytes)) => {
            metrics.payload_bytes.record(bytes as f64);
            debug!(
                target: "sequencer::builder",
                payload_id = %payload_id,
                index,
                block_number,
                transactions = tracker.interval_transactions.len(),
                subscribers = count,
                is_last,
                "emitted flashblock"
            );
        }
        Err(e) => {
            record_publish_error("serialize");
            warn!(
                target: "sequencer::builder",
                payload_id = %payload_id,
                index,
                error = %e,
                "failed to publish flashblock"
            );
        }
    }
}
