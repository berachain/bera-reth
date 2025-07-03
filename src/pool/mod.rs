use crate::{chainspec::BerachainChainSpec, transaction::BerachainTxEnvelope};
use alloy_consensus::transaction::Recovered;
use alloy_eips::{
    eip4844::{BlobAndProofV1, BlobAndProofV2},
    eip7594::BlobTransactionSidecarVariant,
};
use alloy_primitives::{Address, B256, TxHash, U256};
use reth::{
    api::NodeTypes,
    chainspec::EthereumHardforks,
    network::types::HandleMempoolData,
    transaction_pool::{
        AllPoolTransactions, AllTransactionsEvents, BestTransactions, BestTransactionsAttributes,
        BlobStoreError, BlockInfo, GetPooledTransactionLimit, NewBlobSidecar, NewTransactionEvent,
        PoolResult, PoolSize, PoolTransaction, Priority, PropagatedTransactions, TransactionEvents,
        TransactionListenerKind, TransactionOrdering, TransactionOrigin, TransactionPool,
        ValidPoolTransaction,
    },
};
use reth_node_api::FullNodeTypes;
use reth_node_builder::{BuilderContext, components::PoolBuilder};
use reth_primitives_traits::NodePrimitives;
use std::{collections::HashSet, fmt::Debug, future::Future, sync::Arc};
use tokio::sync::mpsc::Receiver;

#[derive(Debug, Default)]
pub struct BerachainPoolBuilder;

impl<Types, Node> PoolBuilder<Node> for BerachainPoolBuilder
where
    Types: NodeTypes<
            ChainSpec = BerachainChainSpec,
            Primitives: NodePrimitives<SignedTx = BerachainTxEnvelope>,
        >,
    Node: FullNodeTypes<Types = Types>,
{
    type Pool = BerachainPool<BerachainPooledTransaction>;

    async fn build_pool(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::Pool> {
        todo!("Build BerachainPool with custom transaction validation")
    }
}

/// Berachain pooled transaction wrapper
pub type BerachainPooledTransaction = BerachainTxEnvelope;

/// Custom transaction pool for Berachain supporting BerachainTxEnvelope
#[derive(Debug, Clone)]
pub struct BerachainPool<T> {
    inner: Arc<BerachainPoolInner<T>>,
}

#[derive(Debug)]
struct BerachainPoolInner<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> BerachainPool<T> {
    pub fn new() -> Self {
        Self { inner: Arc::new(BerachainPoolInner { _phantom: std::marker::PhantomData }) }
    }
}

impl<T: reth::transaction_pool::EthPoolTransaction> TransactionPool for BerachainPool<T> {
    type Transaction = T;

    fn pool_size(&self) -> PoolSize {
        todo!()
    }

    fn block_info(&self) -> BlockInfo {
        todo!()
    }

    fn add_transaction_and_subscribe(
        &self,
        origin: TransactionOrigin,
        transaction: Self::Transaction,
    ) -> impl Future<Output = PoolResult<TransactionEvents>> + Send {
        async { todo!("Add transaction and return event stream") }
    }

    fn add_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: Self::Transaction,
    ) -> impl Future<Output = PoolResult<TxHash>> + Send {
        async { todo!("Validate and add transaction to pool") }
    }

    fn add_transactions(
        &self,
        origin: TransactionOrigin,
        transactions: Vec<Self::Transaction>,
    ) -> impl Future<Output = Vec<PoolResult<TxHash>>> + Send {
        async { todo!("Batch add transactions") }
    }

    fn transaction_event_listener(&self, tx_hash: TxHash) -> Option<TransactionEvents> {
        todo!()
    }

    fn all_transactions_event_listener(&self) -> AllTransactionsEvents<Self::Transaction> {
        todo!()
    }

    fn pending_transactions_listener_for(&self, kind: TransactionListenerKind) -> Receiver<TxHash> {
        todo!()
    }

    fn blob_transaction_sidecars_listener(&self) -> Receiver<NewBlobSidecar> {
        todo!()
    }

    fn new_transactions_listener_for(
        &self,
        kind: TransactionListenerKind,
    ) -> Receiver<NewTransactionEvent<Self::Transaction>> {
        todo!()
    }

    fn pooled_transaction_hashes(&self) -> Vec<TxHash> {
        todo!()
    }

    fn pooled_transaction_hashes_max(&self, max: usize) -> Vec<TxHash> {
        todo!()
    }

    fn pooled_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn pooled_transactions_max(
        &self,
        max: usize,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_pooled_transaction_elements(
        &self,
        tx_hashes: Vec<TxHash>,
        limit: GetPooledTransactionLimit,
    ) -> Vec<<Self::Transaction as PoolTransaction>::Pooled> {
        todo!()
    }

    fn get_pooled_transaction_element(
        &self,
        tx_hash: TxHash,
    ) -> Option<Recovered<<Self::Transaction as PoolTransaction>::Pooled>> {
        todo!()
    }

    fn best_transactions(
        &self,
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Transaction>>>> {
        todo!()
    }

    fn best_transactions_with_attributes(
        &self,
        best_transactions_attributes: BestTransactionsAttributes,
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Transaction>>>> {
        todo!()
    }

    fn pending_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn pending_transactions_max(
        &self,
        max: usize,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn queued_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn all_transactions(&self) -> AllPoolTransactions<Self::Transaction> {
        todo!()
    }

    fn remove_transactions(
        &self,
        hashes: Vec<TxHash>,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn remove_transactions_and_descendants(
        &self,
        hashes: Vec<TxHash>,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn remove_transactions_by_sender(
        &self,
        sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn retain_unknown<A>(&self, announcement: &mut A)
    where
        A: HandleMempoolData,
    {
        todo!()
    }

    fn get(&self, tx_hash: &TxHash) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_all(&self, txs: Vec<TxHash>) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn on_propagated(&self, txs: PropagatedTransactions) {
        todo!()
    }

    fn get_transactions_by_sender(
        &self,
        sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_pending_transactions_with_predicate(
        &self,
        predicate: impl FnMut(&ValidPoolTransaction<Self::Transaction>) -> bool,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_pending_transactions_by_sender(
        &self,
        sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_queued_transactions_by_sender(
        &self,
        sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_highest_transaction_by_sender(
        &self,
        sender: Address,
    ) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_highest_consecutive_transaction_by_sender(
        &self,
        sender: Address,
        on_chain_nonce: u64,
    ) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_transaction_by_sender_and_nonce(
        &self,
        sender: Address,
        nonce: u64,
    ) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_transactions_by_origin(
        &self,
        origin: TransactionOrigin,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn get_pending_transactions_by_origin(
        &self,
        origin: TransactionOrigin,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        todo!()
    }

    fn unique_senders(&self) -> HashSet<Address> {
        todo!()
    }

    fn get_blob(
        &self,
        tx_hash: TxHash,
    ) -> Result<Option<Arc<BlobTransactionSidecarVariant>>, BlobStoreError> {
        todo!()
    }

    fn get_all_blobs(
        &self,
        tx_hashes: Vec<TxHash>,
    ) -> Result<Vec<(TxHash, Arc<BlobTransactionSidecarVariant>)>, BlobStoreError> {
        todo!()
    }

    fn get_all_blobs_exact(
        &self,
        tx_hashes: Vec<TxHash>,
    ) -> Result<Vec<Arc<BlobTransactionSidecarVariant>>, BlobStoreError> {
        todo!()
    }

    fn get_blobs_for_versioned_hashes_v1(
        &self,
        versioned_hashes: &[B256],
    ) -> Result<Vec<Option<BlobAndProofV1>>, BlobStoreError> {
        todo!()
    }

    fn get_blobs_for_versioned_hashes_v2(
        &self,
        versioned_hashes: &[B256],
    ) -> Result<Option<Vec<BlobAndProofV2>>, BlobStoreError> {
        todo!()
    }
}

/// Custom transaction ordering for Berachain
#[derive(Debug, Default, Clone)]
pub struct BerachainOrdering<T>(std::marker::PhantomData<T>);

impl<T: reth::transaction_pool::PoolTransaction> TransactionOrdering for BerachainOrdering<T> {
    type PriorityValue = U256;
    type Transaction = T;

    fn priority(
        &self,
        _transaction: &Self::Transaction,
        _base_fee: u64,
    ) -> Priority<Self::PriorityValue> {
        todo!("Implement Berachain-specific transaction ordering")
    }
}
