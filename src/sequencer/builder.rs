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
    node::evm::{
        FlashblockState,
        config::{BerachainEvmConfig, BerachainNextBlockEnvAttributes},
    },
    primitives::BerachainHeader,
    sequencer::{
        signing::{compute_diff_hash, FlashblockSigner},
        SequencerConfig, WebSocketPublisher,
    },
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::{
    Transaction, TxReceipt, EMPTY_OMMER_ROOT_HASH, EMPTY_ROOT_HASH,
    proofs::{calculate_withdrawals_root, ordered_trie_root_with_encoder},
};
use alloy_eips::{eip2718::Encodable2718, eip4895::Withdrawal};
use bytes::BufMut;
use alloy_primitives::{logs_bloom, B256, B64, Bloom, Bytes, Sealable, U256};
use reth::{
    api::{FullNodeTypes, NodeTypes, PayloadBuilderError, PayloadTypes, TxTy},
    chainspec::EthereumHardforks,
    providers::StateProviderFactory,
    revm::{context::Block, database::StateProviderDatabase, State},
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_basic_payload_builder::{
    BuildArguments, BuildOutcome, MissingPayloadBehaviour, PayloadBuilder, PayloadConfig,
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_ethereum_payload_builder::EthereumBuilderConfig;
use reth_ethereum_primitives::Receipt;
use reth_evm::{
    block::{BlockExecutionError, BlockValidationError, CommitChanges},
    execute::{BlockBuilder, BlockBuilderOutcome},
    ConfigureEvm, Evm,
};
use reth_node_builder::{components::PayloadBuilderBuilder, BuilderContext, PayloadBuilderConfig};
use reth_payload_primitives::PayloadBuilderAttributes;
use reth_primitives_traits::transaction::error::InvalidTransactionError;
use reth_transaction_pool::{
    error::InvalidPoolTransactionError, BestTransactions, BestTransactionsAttributes,
    ValidPoolTransaction,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, info, trace, warn};

use crate::transaction::BerachainTxType;

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
        Self { client, pool, evm_config, builder_config, sequencer_config, publisher, deadline }
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
        build_flashblock_payload(
            self.evm_config.clone(),
            self.client.clone(),
            self.pool.clone(),
            self.builder_config.clone(),
            self.sequencer_config.clone(),
            self.publisher.clone(),
            self.deadline,
            args,
            |attributes| self.pool.best_transactions_with_attributes(attributes),
        )
    }

    fn on_missing_payload(
        &self,
        _args: BuildArguments<Self::Attributes, Self::BuiltPayload>,
    ) -> MissingPayloadBehaviour<Self::BuiltPayload> {
        MissingPayloadBehaviour::AwaitInProgress
    }

    fn build_empty_payload(
        &self,
        config: PayloadConfig<BerachainPayloadBuilderAttributes, BerachainHeader>,
    ) -> Result<BerachainBuiltPayload, PayloadBuilderError> {
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

    /// Compute the receipts root from accumulated receipts.
    fn receipts_root(&self) -> B256 {
        Receipt::calculate_receipt_root_no_memo(&self.receipts)
    }

    /// Compute the logs bloom from accumulated receipts.
    fn logs_bloom(&self) -> Bloom {
        logs_bloom(self.receipts.iter().flat_map(|r| r.logs()))
    }

    /// Clear interval transactions for next flashblock.
    fn clear_interval(&mut self) {
        self.interval_transactions.clear();
    }
}

use reth_storage_api::{HashedPostStateProvider, StateRootProvider};

/// Compute the intermediate state root from a block builder.
///
/// Merges pending state transitions and computes the state root. Requires
/// the builder's database to implement [`FlashblockState`].
fn compute_intermediate_state_root<B, S>(
    builder: &mut B,
    state_provider: &S,
) -> reth_storage_api::errors::ProviderResult<B256>
where
    B: BlockBuilder,
    S: StateRootProvider + HashedPostStateProvider,
    <<B::Executor as reth_evm::block::BlockExecutor>::Evm as Evm>::DB: FlashblockState,
{
    let db = builder.evm_mut().db_mut();
    db.merge_transitions_for_flashblock();
    let hashed_state = state_provider.hashed_post_state(db.bundle_state());
    state_provider.state_root(hashed_state)
}

/// Build a payload while emitting flashblocks at regular intervals.
#[allow(clippy::too_many_arguments)]
fn build_flashblock_payload<Client, Pool, F>(
    evm_config: BerachainEvmConfig,
    client: Client,
    _pool: Pool,
    builder_config: EthereumBuilderConfig,
    sequencer_config: SequencerConfig,
    publisher: Arc<WebSocketPublisher>,
    deadline: Duration,
    args: BuildArguments<BerachainPayloadBuilderAttributes, BerachainBuiltPayload>,
    best_txs: F,
) -> Result<BuildOutcome<BerachainBuiltPayload>, PayloadBuilderError>
where
    Client: StateProviderFactory + ChainSpecProvider<ChainSpec = BerachainChainSpec>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = BerachainTxEnvelope>>,
    F: FnOnce(BestTransactionsAttributes) -> BestTransactionsIter<Pool>,
{
    let BuildArguments { mut cached_reads, config, cancel, best_payload: _ } = args;
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
        warn!(target: "sequencer::builder", %err, "failed to apply pre-execution changes");
        PayloadBuilderError::Internal(err.into())
    })?;

    // Check if Prague3 is active
    if chain_spec.is_prague3_active_at_timestamp(attributes.timestamp()) {
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
    let mut flashblock_index = 0u64;
    let mut last_flashblock_time = Instant::now();
    let interval = sequencer_config.interval;
    let build_start_time = Instant::now();

    // Helper to compute state root, emit flashblock, and handle errors.
    // Returns true if emission succeeded.
    let try_emit_flashblock =
        |builder: &mut _, flashblock_index: u64, tracker: &_, is_last: bool| -> bool {
            match compute_intermediate_state_root(builder, &state_provider) {
                Ok(state_root) => {
                    emit_flashblock(
                        &publisher,
                        payload_id,
                        flashblock_index,
                        &base,
                        flashblock_index == 0,
                        tracker,
                        block_number,
                        &sequencer_config.signer,
                        state_root,
                        &withdrawals,
                        is_last,
                    );
                    true
                }
                Err(e) => {
                    warn!(
                        target: "sequencer::builder",
                        payload_id = %payload_id,
                        index = flashblock_index,
                        error = %e,
                        "skipping flashblock emission due to state root computation failure"
                    );
                    false
                }
            }
        };

    // Main transaction execution loop with flashblock emission.
    // Flashblocks are emitted at regular intervals (~200ms) regardless of transaction activity.
    // Empty flashblocks serve as heartbeats, allowing subscribers to detect liveness and
    // track the current state even when no transactions are being processed.
    loop {
        // Check if cancelled (getPayload called).
        // TODO: detect orphaned builds on new forkchoiceUpdated.
        if cancel.is_cancelled() {
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
            if try_emit_flashblock(&mut builder, flashblock_index, &tracker, false) {
                flashblock_index += 1;
                tracker.clear_interval();
            }
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

        // Execute the transaction and capture the result
        let mut execution_logs = Vec::new();

        let result = builder.execute_transaction_with_commit_condition(tx.clone(), |exec_result| {
            // Capture execution result before commit decision
            // ExecutionResult contains the output with logs
            if exec_result.is_success() {
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
            success: true,
            cumulative_gas_used: tracker.cumulative_gas_used + gas_used,
            logs: execution_logs,
        };
        tracker.receipts.push(receipt);

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
    try_emit_flashblock(&mut builder, flashblock_index, &tracker, true);

    // Finalize the block
    let BlockBuilderOutcome { execution_result, block, .. } = builder.finish(&state_provider)?;

    let requests = chain_spec
        .is_prague_active_at_timestamp(attributes.timestamp())
        .then_some(execution_result.requests);

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

    let payload = BerachainBuiltPayload::new(payload_id, sealed_block, tracker.total_fees, requests);

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
    block_number: u64,
    signer: &FlashblockSigner,
    state_root: B256,
    withdrawals: &[Withdrawal],
    is_last: bool,
) {
    // Compute roots from accumulated receipts
    let receipts_root = tracker.receipts_root();
    let logs_bloom = tracker.logs_bloom();

    // Compute transactions root from all transactions (already encoded)
    let transactions_root = ordered_trie_root_with_encoder(
        &tracker.all_transactions,
        |tx, buf| buf.put_slice(tx.as_ref()),
    );

    // Withdrawals are included in first flashblock only (index 0)
    let (diff_withdrawals, withdrawals_root) = if include_base_in_payload {
        let root = if withdrawals.is_empty() {
            EMPTY_ROOT_HASH
        } else {
            calculate_withdrawals_root(withdrawals)
        };
        (withdrawals.to_vec(), root)
    } else {
        (vec![], EMPTY_ROOT_HASH)
    };

    // Construct header to compute block_hash
    let header = BerachainHeader {
        parent_hash: base.parent_hash,
        ommers_hash: EMPTY_OMMER_ROOT_HASH,
        beneficiary: base.fee_recipient,
        state_root,
        transactions_root,
        receipts_root,
        withdrawals_root: Some(withdrawals_root),
        logs_bloom,
        difficulty: U256::ZERO,
        number: block_number,
        gas_limit: base.gas_limit,
        gas_used: tracker.cumulative_gas_used,
        timestamp: base.timestamp,
        mix_hash: base.prev_randao,
        nonce: B64::ZERO,
        base_fee_per_gas: Some(base.base_fee_per_gas.to::<u64>()),
        blob_gas_used: None,
        excess_blob_gas: None,
        parent_beacon_block_root: Some(base.parent_beacon_block_root),
        requests_hash: None,
        prev_proposer_pubkey: base.prev_proposer_pubkey,
        extra_data: base.extra_data.clone(),
    };

    let block_hash = header.hash_slow();

    let diff = BerachainFlashblockPayloadDiff {
        state_root,
        receipts_root,
        logs_bloom,
        gas_used: tracker.cumulative_gas_used,
        block_hash,
        transactions: tracker.interval_transactions.clone(),
        withdrawals: diff_withdrawals,
        withdrawals_root,
        blob_gas_used: None,
    };

    let diff_hash = compute_diff_hash(
        state_root,
        receipts_root,
        logs_bloom.as_slice(),
        tracker.cumulative_gas_used,
        block_hash,
        &tracker.interval_transactions,
    );
    let signature = signer.sign_flashblock(block_number, payload_id, index, diff_hash);

    let flashblock = BerachainFlashblockPayload {
        payload_id,
        index,
        base: if include_base_in_payload { Some(base.clone()) } else { None },
        diff,
        metadata: BerachainFlashblockPayloadMetadata { block_number },
        signature,
        is_last,
    };

    match publisher.publish(&flashblock) {
        Ok(count) => {
            debug!(
                target: "sequencer::builder",
                payload_id = %payload_id,
                index,
                block_number,
                block_hash = %block_hash,
                transactions = tracker.interval_transactions.len(),
                subscribers = count,
                is_last,
                "emitted flashblock"
            );
        }
        Err(e) => {
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
