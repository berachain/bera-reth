use alloy_consensus::BlockHeader;
use alloy_network::{
    BuildResult, Ethereum, Network, NetworkWallet, TransactionBuilder, TransactionBuilderError,
};
use core::fmt;
use derive_more::Deref;
use reth::{
    providers::BlockReader,
    rpc::eth::{core::EthApiInner, helpers::types::EthRpcConverter},
    transaction_pool::PoolTransaction,
};
use reth_rpc_eth_api::{FullEthApiTypes, RpcReceipt};

use crate::transaction::{BerachainTxEnvelope, TxTypeCustom};
use alloy_consensus::transaction::TransactionMeta;
use alloy_eips::{BlockId, eip2930::AccessList};
use alloy_primitives::{Address, B256, Bytes, ChainId, TxKind, U256};
use alloy_rpc_types_eth::{Transaction, TransactionReceipt, TransactionRequest};
use reth::{
    chainspec::EthereumHardforks,
    network::NetworkInfo,
    providers::{
        BlockNumReader, BlockReaderIdExt, NodePrimitivesProvider, ProviderBlock, ProviderError,
        ProviderHeader, ProviderReceipt, ProviderTx, ReceiptProvider, StageCheckpointReader,
        StateProviderFactory, TransactionsProvider,
    },
    revm::{context::TxEnv, interpreter::Host},
    rpc::{
        compat::{RpcConvert, RpcTypes},
        eth::DevSigner,
    },
    tasks::{
        TaskSpawner,
        pool::{BlockingTaskGuard, BlockingTaskPool},
    },
    transaction_pool::TransactionPool,
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_ethereum_primitives::Receipt;
use reth_evm::{
    ConfigureEvm, EvmFactory, NextBlockEnvAttributes, TxEnvFor, block::BlockExecutorFactory,
};
use reth_primitives_traits::{NodePrimitives, SealedHeader};
use reth_rpc_eth_api::{
    EthApiTypes, RpcNodeCore, RpcNodeCoreExt,
    helpers::{
        AddDevSigners, Call, EthApiSpec, EthBlocks, EthCall, EthFees, EthSigner, EthState,
        EthTransactions, LoadBlock, LoadFee, LoadPendingBlock, LoadReceipt, LoadState,
        LoadTransaction, SpawnBlocking, Trace, estimate::EstimateCall,
    },
};
use reth_rpc_eth_types::{
    EthApiError, EthStateCache, FeeHistoryCache, GasPriceOracle, PendingBlock, error::FromEvmError,
};
use std::sync::Arc;

impl fmt::Display for TxTypeCustom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

pub enum BerachainTypedTransaction {}

impl From<BerachainTxEnvelope> for BerachainTypedTransaction {
    fn from(value: BerachainTxEnvelope) -> Self {
        todo!()
    }
}

impl From<BerachainTxEnvelope> for alloy_rpc_types_eth::transaction::TransactionRequest {
    fn from(value: BerachainTxEnvelope) -> Self {
        todo!()
    }
}
impl From<BerachainTypedTransaction> for alloy_rpc_types_eth::transaction::TransactionRequest {
    fn from(value: BerachainTypedTransaction) -> Self {
        todo!()
    }
}

impl TransactionBuilder<BerachainNetwork> for alloy_rpc_types_eth::transaction::TransactionRequest {
    fn chain_id(&self) -> Option<ChainId> {
        todo!()
    }

    fn set_chain_id(&mut self, chain_id: ChainId) {
        todo!()
    }

    fn nonce(&self) -> Option<u64> {
        todo!()
    }

    fn set_nonce(&mut self, nonce: u64) {
        todo!()
    }

    fn take_nonce(&mut self) -> Option<u64> {
        todo!()
    }

    fn input(&self) -> Option<&Bytes> {
        todo!()
    }

    fn set_input<T: Into<Bytes>>(&mut self, input: T) {
        todo!()
    }

    fn from(&self) -> Option<Address> {
        todo!()
    }

    fn set_from(&mut self, from: Address) {
        todo!()
    }

    fn kind(&self) -> Option<TxKind> {
        todo!()
    }

    fn clear_kind(&mut self) {
        todo!()
    }

    fn set_kind(&mut self, kind: TxKind) {
        todo!()
    }

    fn value(&self) -> Option<U256> {
        todo!()
    }

    fn set_value(&mut self, value: U256) {
        todo!()
    }

    fn gas_price(&self) -> Option<u128> {
        todo!()
    }

    fn set_gas_price(&mut self, gas_price: u128) {
        todo!()
    }

    fn max_fee_per_gas(&self) -> Option<u128> {
        todo!()
    }

    fn set_max_fee_per_gas(&mut self, max_fee_per_gas: u128) {
        todo!()
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        todo!()
    }

    fn set_max_priority_fee_per_gas(&mut self, max_priority_fee_per_gas: u128) {
        todo!()
    }

    fn gas_limit(&self) -> Option<u64> {
        todo!()
    }

    fn set_gas_limit(&mut self, gas_limit: u64) {
        todo!()
    }

    fn access_list(&self) -> Option<&AccessList> {
        todo!()
    }

    fn set_access_list(&mut self, access_list: AccessList) {
        todo!()
    }

    fn complete_type(
        &self,
        ty: <BerachainNetwork as Network>::TxType,
    ) -> Result<(), Vec<&'static str>> {
        todo!()
    }

    fn can_submit(&self) -> bool {
        todo!()
    }

    fn can_build(&self) -> bool {
        todo!()
    }

    fn output_tx_type(&self) -> <BerachainNetwork as Network>::TxType {
        todo!()
    }

    fn output_tx_type_checked(&self) -> Option<<BerachainNetwork as Network>::TxType> {
        todo!()
    }

    fn prep_for_submission(&mut self) {
        todo!()
    }

    fn build_unsigned(
        self,
    ) -> BuildResult<<BerachainNetwork as Network>::UnsignedTx, BerachainNetwork> {
        todo!()
    }

    async fn build<W: NetworkWallet<BerachainNetwork>>(
        self,
        wallet: &W,
    ) -> Result<<BerachainNetwork as Network>::TxEnvelope, TransactionBuilderError<BerachainNetwork>>
    {
        todo!()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BerachainNetwork {
    _private: (),
}

impl Network for BerachainNetwork {
    type TxType = TxTypeCustom;

    type TxEnvelope = BerachainTxEnvelope;

    type UnsignedTx = BerachainTypedTransaction;

    type ReceiptEnvelope = alloy_consensus::ReceiptEnvelope;

    type Header = alloy_consensus::Header;

    type TransactionRequest = alloy_rpc_types_eth::transaction::TransactionRequest;

    type TransactionResponse = alloy_rpc_types_eth::Transaction<BerachainTxEnvelope>;

    type ReceiptResponse = alloy_rpc_types_eth::TransactionReceipt;

    type HeaderResponse = alloy_rpc_types_eth::Header;

    type BlockResponse = alloy_rpc_types_eth::Block<Transaction<BerachainTxEnvelope>>;
}

#[derive(Deref)]
pub struct BerachainApi<Provider: BlockReader, Pool, Network, EvmConfig> {
    /// All nested fields bundled together.
    #[deref]
    pub(super) inner: Arc<EthApiInner<Provider, Pool, Network, EvmConfig>>,
    /// Transaction RPC response builder.
    pub tx_resp_builder: EthRpcConverter,
}

impl<Provider, Pool, Network, EvmConfig> Clone for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Provider: BlockReader,
    Self: Send + Sync,
{
    fn clone(&self) -> Self {
        todo!()
    }
}

impl<Provider, Pool, Network, EvmConfig> EthApiTypes
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: Send + Sync,
    Provider: BlockReader,
{
    type Error = EthApiError;

    // TODO: Change
    type NetworkTypes = BerachainNetwork;
    type RpcConvert = EthRpcConverter;

    fn tx_resp_builder(&self) -> &Self::RpcConvert {
        &self.tx_resp_builder
    }
}

impl<Provider, Pool, Network, EvmConfig> RpcNodeCore
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Provider: BlockReader + NodePrimitivesProvider + Clone + Unpin,
    Pool: Send + Sync + Clone + Unpin,
    Network: Send + Sync + Clone,
    EvmConfig: Send + Sync + Clone + Unpin,
{
    type Primitives = Provider::Primitives;
    type Provider = Provider;
    type Pool = Pool;
    type Evm = EvmConfig;
    type Network = Network;
    type PayloadBuilder = ();

    fn pool(&self) -> &Self::Pool {
        self.inner.pool()
    }

    fn evm_config(&self) -> &Self::Evm {
        self.inner.evm_config()
    }

    fn network(&self) -> &Self::Network {
        self.inner.network()
    }

    fn payload_builder(&self) -> &Self::PayloadBuilder {
        &()
    }

    fn provider(&self) -> &Self::Provider {
        self.inner.provider()
    }
}

impl<Provider, Pool, Network, EvmConfig> RpcNodeCoreExt
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Provider: BlockReader + NodePrimitivesProvider + Clone + Unpin,
    Pool: Send + Sync + Clone + Unpin,
    Network: Send + Sync + Clone,
    EvmConfig: Send + Sync + Clone + Unpin,
{
    #[inline]
    fn cache(&self) -> &EthStateCache<ProviderBlock<Provider>, ProviderReceipt<Provider>> {
        self.inner.cache()
    }
}

impl<Provider, Pool, Network, EvmConfig> std::fmt::Debug
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Provider: BlockReader,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EthApi").finish_non_exhaustive()
    }
}

impl<Provider, Pool, Network, EvmConfig> SpawnBlocking
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: Clone + Send + Sync + 'static,
    Provider: BlockReader,
{
    #[inline]
    fn io_task_spawner(&self) -> impl TaskSpawner {
        self.inner.task_spawner()
    }

    #[inline]
    fn tracing_task_pool(&self) -> &BlockingTaskPool {
        self.inner.blocking_task_pool()
    }

    #[inline]
    fn tracing_task_guard(&self) -> &BlockingTaskGuard {
        self.inner.blocking_task_guard()
    }
}

impl<Provider, Pool, Network, EvmConfig> AddDevSigners
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Provider: BlockReader,
{
    fn with_dev_accounts(&self) {
        *self.inner.signers().write() = DevSigner::random_signers(20)
    }
}

impl<Provider, Pool, Network, EvmConfig> EthTransactions
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadTransaction<Provider: BlockReaderIdExt>,
    Provider: BlockReader<Transaction = ProviderTx<Self::Provider>>,
{
    #[inline]
    fn signers(&self) -> &parking_lot::RwLock<Vec<Box<dyn EthSigner<ProviderTx<Self::Provider>>>>> {
        self.inner.signers()
    }

    /// Decodes and recovers the transaction and submits it to the pool.
    ///
    /// Returns the hash of the transaction.
    async fn send_raw_transaction(&self, tx: Bytes) -> Result<B256, Self::Error> {
        todo!()
    }
}

impl<Provider, Pool, Network, EvmConfig> LoadTransaction
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: SpawnBlocking
        + FullEthApiTypes
        + RpcNodeCoreExt<Provider: TransactionsProvider, Pool: TransactionPool>,
    Provider: BlockReader,
{
}

impl<Provider, Pool, Network, EvmConfig> LoadReceipt
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: RpcNodeCoreExt<
        Provider: TransactionsProvider<Transaction = BerachainTxEnvelope>
                      + ReceiptProvider<Receipt = reth_ethereum_primitives::Receipt>,
    >,
    Provider: BlockReader + ChainSpecProvider,
{
    async fn build_transaction_receipt(
        &self,
        tx: BerachainTxEnvelope,
        meta: TransactionMeta,
        receipt: Receipt,
    ) -> Result<RpcReceipt<Self::NetworkTypes>, Self::Error> {
        todo!()
    }
}

impl<Provider, Pool, Network, EvmConfig> EthApiSpec
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: RpcNodeCore<
            Provider: ChainSpecProvider<ChainSpec: EthereumHardforks>
                          + BlockNumReader
                          + StageCheckpointReader,
            Network: NetworkInfo,
        >,
    Provider: BlockReader,
{
    type Transaction = ProviderTx<Provider>;

    fn starting_block(&self) -> U256 {
        self.inner.starting_block()
    }

    fn signers(
        &self,
    ) -> &parking_lot::RwLock<Vec<Box<dyn reth_rpc_eth_api::helpers::EthSigner<Self::Transaction>>>>
    {
        self.inner.signers()
    }
}

impl<Provider, Pool, Network, EvmConfig> EthBlocks
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadBlock<
            Error = EthApiError,
            NetworkTypes: RpcTypes<Receipt = TransactionReceipt>,
            RpcConvert: RpcConvert<Network = Self::NetworkTypes>,
            Provider: BlockReader<
                Transaction = BerachainTxEnvelope,
                Receipt = reth_ethereum_primitives::Receipt,
            >,
        >,
    Provider: BlockReader + ChainSpecProvider,
{
    async fn block_receipts(
        &self,
        block_id: BlockId,
    ) -> Result<Option<Vec<RpcReceipt<Self::NetworkTypes>>>, Self::Error>
    where
        Self: LoadReceipt,
    {
        todo!()
    }
}

impl<Provider, Pool, Network, EvmConfig> LoadBlock
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadPendingBlock
        + SpawnBlocking
        + RpcNodeCoreExt<
            Pool: TransactionPool<
                Transaction: PoolTransaction<Consensus = ProviderTx<Self::Provider>>,
            >,
            Primitives: NodePrimitives<SignedTx = ProviderTx<Self::Provider>>,
            Evm = EvmConfig,
        >,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm<Primitives = <Self as RpcNodeCore>::Primitives>,
{
}

impl<Provider, Pool, Network, EvmConfig> EthCall
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: EstimateCall
        + LoadPendingBlock
        + FullEthApiTypes
        + RpcNodeCoreExt<
            Pool: TransactionPool<
                Transaction: PoolTransaction<Consensus = ProviderTx<Self::Provider>>,
            >,
            Primitives: NodePrimitives<SignedTx = ProviderTx<Self::Provider>>,
            Evm = EvmConfig,
        >,
    EvmConfig: ConfigureEvm<Primitives = <Self as RpcNodeCore>::Primitives>,
    Provider: BlockReader,
{
}

impl<Provider, Pool, Network, EvmConfig> EstimateCall
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: Call,
    Provider: BlockReader,
{
}

impl<Provider, Pool, Network, EvmConfig> Call for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadState<
            Evm: ConfigureEvm<
                BlockExecutorFactory: BlockExecutorFactory<EvmFactory: EvmFactory<Tx = TxEnv>>,
                Primitives: NodePrimitives<
                    BlockHeader = ProviderHeader<Self::Provider>,
                    SignedTx = ProviderTx<Self::Provider>,
                >,
            >,
            RpcConvert: RpcConvert<TxEnv = TxEnvFor<Self::Evm>, Network = Self::NetworkTypes>,
            NetworkTypes: RpcTypes<TransactionRequest: From<TransactionRequest>>,
            Error: FromEvmError<Self::Evm>
                       + From<<Self::RpcConvert as RpcConvert>::Error>
                       + From<ProviderError>,
        > + SpawnBlocking,
    Provider: BlockReader,
{
    #[inline]
    fn call_gas_limit(&self) -> u64 {
        self.inner.gas_cap()
    }

    #[inline]
    fn max_simulate_blocks(&self) -> u64 {
        self.inner.max_simulate_blocks()
    }
}

impl<Provider, Pool, Network, EvmConfig> EthFees
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadFee<
        Provider: ChainSpecProvider<
            ChainSpec: EthChainSpec<Header = ProviderHeader<Self::Provider>>,
        >,
    >,
    Provider: BlockReader,
{
}

impl<Provider, Pool, Network, EvmConfig> EthState
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadState + SpawnBlocking,
    Provider: BlockReader,
{
    fn max_proof_window(&self) -> u64 {
        self.inner.eth_proof_window()
    }
}

impl<Provider, Pool, Network, EvmConfig> Trace for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadState<
            Provider: BlockReader,
            Evm: ConfigureEvm<
                Primitives: NodePrimitives<
                    BlockHeader = ProviderHeader<Self::Provider>,
                    SignedTx = ProviderTx<Self::Provider>,
                >,
            >,
            Error: FromEvmError<Self::Evm>,
        >,
    Provider: BlockReader,
{
}

impl<Provider, Pool, Network, EvmConfig> LoadState
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: RpcNodeCoreExt<
            Provider: BlockReader
                          + StateProviderFactory
                          + ChainSpecProvider<ChainSpec: EthereumHardforks>,
            Pool: TransactionPool,
        >,
    Provider: BlockReader,
{
}

impl<Provider, Pool, Network, EvmConfig> LoadFee
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: LoadBlock<Provider = Provider>,
    Provider: BlockReaderIdExt
        + ChainSpecProvider<ChainSpec: EthChainSpec + EthereumHardforks>
        + StateProviderFactory,
{
    #[inline]
    fn gas_oracle(&self) -> &GasPriceOracle<Self::Provider> {
        self.inner.gas_oracle()
    }

    #[inline]
    fn fee_history_cache(&self) -> &FeeHistoryCache<ProviderHeader<Provider>> {
        self.inner.fee_history_cache()
    }
}

impl<Provider, Pool, Network, EvmConfig> LoadPendingBlock
    for BerachainApi<Provider, Pool, Network, EvmConfig>
where
    Self: SpawnBlocking<
            NetworkTypes: RpcTypes<
                Header = alloy_rpc_types_eth::Header<ProviderHeader<Self::Provider>>,
            >,
            Error: FromEvmError<Self::Evm>,
            RpcConvert: RpcConvert<Network = Self::NetworkTypes>,
        > + RpcNodeCore<
            Provider: BlockReaderIdExt<Receipt = Provider::Receipt, Block = Provider::Block>
                          + ChainSpecProvider<ChainSpec: EthChainSpec + EthereumHardforks>
                          + StateProviderFactory,
            Pool: TransactionPool<
                Transaction: PoolTransaction<Consensus = ProviderTx<Self::Provider>>,
            >,
            Evm: ConfigureEvm<
                Primitives = <Self as RpcNodeCore>::Primitives,
                NextBlockEnvCtx: From<NextBlockEnvAttributes>,
            >,
            Primitives: NodePrimitives<
                BlockHeader = ProviderHeader<Self::Provider>,
                SignedTx = ProviderTx<Self::Provider>,
                Receipt = ProviderReceipt<Self::Provider>,
                Block = ProviderBlock<Self::Provider>,
            >,
        >,
    Provider: BlockReader,
{
    #[inline]
    fn pending_block(
        &self,
    ) -> &tokio::sync::Mutex<
        Option<PendingBlock<ProviderBlock<Self::Provider>, ProviderReceipt<Self::Provider>>>,
    > {
        self.inner.pending_block()
    }

    fn next_env_attributes(
        &self,
        parent: &SealedHeader<ProviderHeader<Self::Provider>>,
    ) -> Result<<Self::Evm as reth_evm::ConfigureEvm>::NextBlockEnvCtx, Self::Error> {
        Ok(NextBlockEnvAttributes {
            timestamp: parent.timestamp().saturating_add(12),
            suggested_fee_recipient: parent.beneficiary(),
            prev_randao: B256::random(),
            gas_limit: parent.gas_limit(),
            parent_beacon_block_root: parent.parent_beacon_block_root().map(|_| B256::ZERO),
            withdrawals: parent.withdrawals_root().map(|_| Default::default()),
        }
        .into())
    }
}
