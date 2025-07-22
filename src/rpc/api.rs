use crate::{
    primitives::BerachainHeader,
    rpc::receipt::BerachainReceiptEnvelope,
    transaction::{BerachainTxEnvelope, BerachainTxType, POL_TX_TYPE},
};
use alloy_consensus::{Transaction, crypto::RecoveryError};
use alloy_eips::eip2930::AccessList;
use alloy_network::{
    BuildResult, Network, NetworkWallet, TransactionBuilder, TransactionBuilderError,
};
use alloy_primitives::{Address, B256, Bytes, ChainId, TxKind, U256};
use alloy_rpc_types_eth::{Transaction as RpcTransaction, TransactionRequest};
use core::fmt;
use derive_more::Deref;
use reth::{
    chainspec::EthereumHardforks,
    network::NetworkInfo,
    providers::{
        BlockNumReader, BlockReader, BlockReaderIdExt, NodePrimitivesProvider, ProviderBlock,
        ProviderError, ProviderHeader, ProviderReceipt, ProviderTx, StageCheckpointReader,
        StateProviderFactory, TransactionsProvider,
    },
    rpc::compat::{RpcConvert, RpcTypes},
    tasks::{
        TaskSpawner,
        pool::{BlockingTaskGuard, BlockingTaskPool},
    },
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_evm::{ConfigureEvm, TxEnvFor};
use reth_primitives_traits::NodePrimitives;
use reth_rpc::eth::DevSigner;
use reth_rpc_convert::SignableTxRequest;
use reth_rpc_eth_api::{
    EthApiTypes, FromEthApiError, FullEthApiTypes, RpcNodeCore, RpcNodeCoreExt,
    helpers::{
        AddDevSigners, Call, EthApiSpec, EthBlocks, EthCall, EthFees, EthState, EthTransactions,
        LoadBlock, LoadFee, LoadPendingBlock, LoadReceipt, LoadState, LoadTransaction,
        SpawnBlocking, Trace,
        estimate::EstimateCall,
        pending_block::PendingEnvBuilder,
        spec::{SignersForApi, SignersForRpc},
    },
};
use reth_rpc_eth_types::{
    EthApiError, EthStateCache, FeeHistoryCache, GasPriceOracle, PendingBlock, error::FromEvmError,
    utils::recover_raw_transaction,
};
use reth_transaction_pool::TransactionOrigin;

impl fmt::Display for BerachainTxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ethereum(tx) => tx.fmt(f),
            Self::Berachain => write!(f, "BRIP-0004"),
        }
    }
}

impl From<BerachainTxEnvelope> for BerachainTxType {
    fn from(value: BerachainTxEnvelope) -> Self {
        match value {
            BerachainTxEnvelope::Ethereum(tx) => Self::Ethereum(tx.tx_type()),
            BerachainTxEnvelope::Berachain(_) => Self::Berachain,
        }
    }
}

impl From<BerachainTxEnvelope> for TransactionRequest {
    fn from(value: BerachainTxEnvelope) -> Self {
        match value {
            BerachainTxEnvelope::Ethereum(tx) => Self {
                to: Some(tx.kind()),
                gas: tx.gas_limit().into(),
                gas_price: tx.gas_price(),
                max_fee_per_gas: Some(tx.max_fee_per_gas()),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas(),
                value: Some(tx.value()),
                input: Some(tx.input().clone()).into(),
                nonce: Some(tx.nonce()),
                chain_id: tx.chain_id(),
                access_list: tx.access_list().cloned(),
                transaction_type: Some(tx.tx_type() as u8),
                ..Default::default()
            },
            BerachainTxEnvelope::Berachain(pol_tx) => Self {
                to: Some(pol_tx.to.into()),
                gas: Some(pol_tx.gas_limit),
                gas_price: Some(pol_tx.gas_price),
                value: Some(pol_tx.value()),
                input: Some(pol_tx.input().clone()).into(),
                nonce: Some(pol_tx.nonce()),
                chain_id: pol_tx.chain_id(),
                from: Some(pol_tx.from),
                ..Default::default()
            },
        }
    }
}
impl From<BerachainTxType> for TransactionRequest {
    fn from(value: BerachainTxType) -> Self {
        Self {
            transaction_type: Some(match value {
                BerachainTxType::Ethereum(tx_type) => tx_type as u8,
                BerachainTxType::Berachain => POL_TX_TYPE,
            }),
            ..Default::default()
        }
    }
}

impl TransactionBuilder<BerachainNetwork> for TransactionRequest {
    fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }

    fn set_chain_id(&mut self, chain_id: ChainId) {
        self.chain_id = Some(chain_id);
    }

    fn nonce(&self) -> Option<u64> {
        self.nonce
    }

    fn set_nonce(&mut self, nonce: u64) {
        self.nonce = Some(nonce);
    }

    fn take_nonce(&mut self) -> Option<u64> {
        self.nonce.take()
    }

    fn input(&self) -> Option<&Bytes> {
        self.input.input.as_ref()
    }

    fn set_input<T: Into<Bytes>>(&mut self, input: T) {
        self.input.input = Some(input.into());
    }

    fn from(&self) -> Option<Address> {
        self.from
    }

    fn set_from(&mut self, from: Address) {
        self.from = Some(from);
    }

    fn kind(&self) -> Option<TxKind> {
        self.to
    }

    fn clear_kind(&mut self) {
        self.to = None;
    }

    fn set_kind(&mut self, kind: TxKind) {
        self.to = Some(kind);
    }

    fn value(&self) -> Option<U256> {
        self.value
    }

    fn set_value(&mut self, value: U256) {
        self.value = Some(value);
    }

    fn gas_price(&self) -> Option<u128> {
        self.gas_price
    }

    fn set_gas_price(&mut self, gas_price: u128) {
        self.gas_price = Some(gas_price);
    }

    fn max_fee_per_gas(&self) -> Option<u128> {
        self.max_fee_per_gas
    }

    fn set_max_fee_per_gas(&mut self, max_fee_per_gas: u128) {
        self.max_fee_per_gas = Some(max_fee_per_gas);
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.max_priority_fee_per_gas
    }

    fn set_max_priority_fee_per_gas(&mut self, max_priority_fee_per_gas: u128) {
        self.max_priority_fee_per_gas = Some(max_priority_fee_per_gas);
    }

    fn gas_limit(&self) -> Option<u64> {
        self.gas
    }

    fn set_gas_limit(&mut self, gas_limit: u64) {
        self.gas = Some(gas_limit);
    }

    fn access_list(&self) -> Option<&AccessList> {
        self.access_list.as_ref()
    }

    fn set_access_list(&mut self, access_list: AccessList) {
        self.access_list = Some(access_list);
    }

    fn complete_type(
        &self,
        ty: <BerachainNetwork as Network>::TxType,
    ) -> Result<(), Vec<&'static str>> {
        let mut missing = Vec::new();

        if self.from.is_none() {
            missing.push("from");
        }
        if self.to.is_none() {
            missing.push("to");
        }
        if self.gas.is_none() {
            missing.push("gas");
        }

        match ty {
            BerachainTxType::Ethereum(_) => {
                if self.gas_price.is_none() && self.max_fee_per_gas.is_none() {
                    missing.push("gas_price or max_fee_per_gas");
                }
            }
            BerachainTxType::Berachain => {
                if self.gas_price.is_none() {
                    missing.push("gas_price");
                }
            }
        }

        if missing.is_empty() { Ok(()) } else { Err(missing) }
    }

    fn can_submit(&self) -> bool {
        self.from.is_some() &&
            self.to.is_some() &&
            self.gas.is_some() &&
            (self.gas_price.is_some() || self.max_fee_per_gas.is_some())
    }

    fn can_build(&self) -> bool {
        self.to.is_some() &&
            self.gas.is_some() &&
            (self.gas_price.is_some() || self.max_fee_per_gas.is_some())
    }

    fn output_tx_type(&self) -> <BerachainNetwork as Network>::TxType {
        match self.transaction_type {
            Some(POL_TX_TYPE) => BerachainTxType::Berachain,
            Some(ty) => BerachainTxType::Ethereum(
                alloy_consensus::TxType::try_from(ty).unwrap_or(alloy_consensus::TxType::Legacy),
            ),
            None => {
                if self.max_fee_per_gas.is_some() || self.max_priority_fee_per_gas.is_some() {
                    BerachainTxType::Ethereum(alloy_consensus::TxType::Eip1559)
                } else if self.access_list.is_some() {
                    BerachainTxType::Ethereum(alloy_consensus::TxType::Eip2930)
                } else {
                    BerachainTxType::Ethereum(alloy_consensus::TxType::Legacy)
                }
            }
        }
    }

    fn output_tx_type_checked(&self) -> Option<<BerachainNetwork as Network>::TxType> {
        if <Self as TransactionBuilder<BerachainNetwork>>::can_build(self) {
            Some(<Self as TransactionBuilder<BerachainNetwork>>::output_tx_type(self))
        } else {
            None
        }
    }

    fn prep_for_submission(&mut self) {
        if self.nonce.is_none() {
            self.nonce = Some(0);
        }
        if self.value.is_none() {
            self.value = Some(U256::ZERO);
        }
        if self.input.input.is_none() {
            self.input.input = Some(Bytes::new());
        }
    }

    fn build_unsigned(
        self,
    ) -> BuildResult<<BerachainNetwork as Network>::UnsignedTx, BerachainNetwork> {
        Ok(<Self as TransactionBuilder<BerachainNetwork>>::output_tx_type(&self))
    }

    async fn build<W: NetworkWallet<BerachainNetwork>>(
        self,
        _wallet: &W,
    ) -> Result<<BerachainNetwork as Network>::TxEnvelope, TransactionBuilderError<BerachainNetwork>>
    {
        Err(TransactionBuilderError::InvalidTransactionRequest(
            <Self as TransactionBuilder<BerachainNetwork>>::output_tx_type(&self),
            vec!["unsupported"],
        ))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BerachainNetwork {
    _private: (),
}

impl Network for BerachainNetwork {
    type TxType = BerachainTxType;

    type TxEnvelope = BerachainTxEnvelope;

    type UnsignedTx = BerachainTxType;

    type ReceiptEnvelope = BerachainReceiptEnvelope;

    type Header = BerachainHeader;

    type TransactionRequest = TransactionRequest;

    type TransactionResponse = RpcTransaction<BerachainTxEnvelope>;

    type ReceiptResponse = alloy_rpc_types_eth::TransactionReceipt<BerachainReceiptEnvelope>;

    type HeaderResponse = alloy_rpc_types_eth::Header<BerachainHeader>;

    type BlockResponse =
        alloy_rpc_types_eth::Block<Self::TransactionResponse, Self::HeaderResponse>;
}

#[derive(Deref)]
pub struct BerachainApi<
    Provider: BlockReader,
    Pool,
    Network,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
> {
    /// All nested fields bundled together.
    #[deref]
    pub(super) inner: reth_rpc::EthApi<Provider, Pool, Network, EvmConfig, Rpc>,
}

impl<Provider, Pool, Network, EvmConfig, Rpc> Clone
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EthApiTypes
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: Send + Sync,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    type Error = EthApiError;

    type NetworkTypes = BerachainNetwork;
    type RpcConvert = Rpc;

    fn tx_resp_builder(&self) -> &Self::RpcConvert {
        self.inner.tx_resp_builder()
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> RpcNodeCore
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Provider: BlockReader + NodePrimitivesProvider + Clone + Unpin,
    Pool: Send + Sync + Clone + Unpin,
    Network: Send + Sync + Clone,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
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

impl<Provider, Pool, Network, EvmConfig, Rpc> RpcNodeCoreExt
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Provider: BlockReader + NodePrimitivesProvider + Clone + Unpin,
    Pool: Send + Sync + Clone + Unpin,
    Network: Send + Sync + Clone,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    #[inline]
    fn cache(&self) -> &EthStateCache<ProviderBlock<Provider>, ProviderReceipt<Provider>> {
        self.inner.cache()
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> std::fmt::Debug
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EthApi").finish_non_exhaustive()
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> SpawnBlocking
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: EthApiTypes<NetworkTypes = Rpc::Network> + Clone + Send + Sync + 'static,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
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

impl<Provider, Pool, Network, EvmConfig, Rpc> AddDevSigners
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert<Network: RpcTypes<TransactionRequest: SignableTxRequest<ProviderTx<Provider>>>>,
{
    fn with_dev_accounts(&self) {
        *self.inner.signers().write() = DevSigner::random_signers(20)
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EthTransactions
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: LoadTransaction<Provider: BlockReaderIdExt> + EthApiTypes<NetworkTypes = Rpc::Network>,
    Provider: BlockReader<Transaction = ProviderTx<Self::Provider>>,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    #[inline]
    fn signers(&self) -> &SignersForRpc<Self::Provider, Self::NetworkTypes> {
        // SAFETY: This is safe because BerachainNetwork and Rpc have the same TransactionRequest
        // type and both implement RpcTypes. The signatures are compatible.
        self.inner.signers()
    }

    /// Decodes and recovers the transaction and submits it to the pool.
    ///
    /// Returns the hash of the transaction.
    async fn send_raw_transaction(&self, tx: Bytes) -> Result<B256, Self::Error> {
        let recovered = recover_raw_transaction(&tx)?;

        // broadcast raw transaction to subscribers if there is any.
        self.broadcast_raw_transaction(tx);

        let pool_transaction = <Self::Pool as TransactionPool>::Transaction::from_pooled(recovered);

        // submit the transaction to the pool with a `Local` origin
        let hash = self
            .pool()
            .add_transaction(TransactionOrigin::Local, pool_transaction)
            .await
            .map_err(Self::Error::from_eth_err)?;

        Ok(hash)
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> LoadTransaction
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: SpawnBlocking
        + FullEthApiTypes
        + RpcNodeCoreExt<Provider: TransactionsProvider, Pool: TransactionPool>
        + EthApiTypes<NetworkTypes = Rpc::Network>,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> LoadReceipt
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: RpcNodeCoreExt<
            Primitives: NodePrimitives<
                SignedTx = ProviderTx<Self::Provider>,
                Receipt = ProviderReceipt<Self::Provider>,
            >,
        > + EthApiTypes<
            NetworkTypes = Rpc::Network,
            RpcConvert: RpcConvert<
                Network = Rpc::Network,
                Primitives = Self::Primitives,
                Error = Self::Error,
            >,
            Error: From<RecoveryError>,
        >,
    Provider: BlockReader + ChainSpecProvider,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EthApiSpec
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: RpcNodeCore<
            Provider: ChainSpecProvider<ChainSpec: EthereumHardforks>
                          + BlockNumReader
                          + StageCheckpointReader,
            Network: NetworkInfo,
        >,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    type Transaction = ProviderTx<Provider>;
    type Rpc = Rpc::Network;

    fn starting_block(&self) -> U256 {
        self.inner.starting_block()
    }

    fn signers(&self) -> &SignersForApi<Self> {
        self.inner.signers()
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EthBlocks
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: LoadBlock<
            Error = EthApiError,
            NetworkTypes = Rpc::Network,
            RpcConvert: RpcConvert<
                Primitives = Self::Primitives,
                Error = Self::Error,
                Network = Rpc::Network,
            >,
        >,
    Provider: BlockReader + ChainSpecProvider,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> LoadBlock
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
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
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EthCall
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: EstimateCall<NetworkTypes = Rpc::Network>
        + LoadPendingBlock<NetworkTypes = Rpc::Network>
        + FullEthApiTypes<NetworkTypes = Rpc::Network>
        + RpcNodeCoreExt<
            Pool: TransactionPool<
                Transaction: PoolTransaction<Consensus = ProviderTx<Self::Provider>>,
            >,
            Primitives: NodePrimitives<SignedTx = ProviderTx<Self::Provider>>,
            Evm = EvmConfig,
        >,
    EvmConfig: ConfigureEvm<Primitives = <Self as RpcNodeCore>::Primitives>,
    Provider: BlockReader,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EstimateCall
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: Call<NetworkTypes = Rpc::Network>,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> Call
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: LoadState<
            Evm: ConfigureEvm<
                Primitives: NodePrimitives<
                    BlockHeader = ProviderHeader<Self::Provider>,
                    SignedTx = ProviderTx<Self::Provider>,
                >,
            >,
            RpcConvert: RpcConvert<TxEnv = TxEnvFor<Self::Evm>, Network = Rpc::Network>,
            NetworkTypes = Rpc::Network,
            Error: FromEvmError<Self::Evm>
                       + From<<Self::RpcConvert as RpcConvert>::Error>
                       + From<ProviderError>,
        > + SpawnBlocking,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
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

impl<Provider, Pool, Network, EvmConfig, Rpc> EthFees
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: LoadFee<
        Provider: ChainSpecProvider<
            ChainSpec: EthChainSpec<Header = ProviderHeader<Self::Provider>>,
        >,
    >,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> EthState
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: LoadState + SpawnBlocking,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
    fn max_proof_window(&self) -> u64 {
        self.inner.eth_proof_window()
    }
}

impl<Provider, Pool, Network, EvmConfig, Rpc> Trace
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
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
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> LoadState
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: RpcNodeCoreExt<
            Provider: BlockReader
                          + StateProviderFactory
                          + ChainSpecProvider<ChainSpec: EthereumHardforks>,
            Pool: TransactionPool,
        > + EthApiTypes<NetworkTypes = Rpc::Network>,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
{
}

impl<Provider, Pool, Network, EvmConfig, Rpc> LoadFee
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: LoadBlock<Provider = Provider>,
    Provider: BlockReaderIdExt
        + ChainSpecProvider<ChainSpec: EthChainSpec + EthereumHardforks>
        + StateProviderFactory,
    EvmConfig: ConfigureEvm,
    Rpc: RpcConvert,
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

impl<Provider, Pool, Network, EvmConfig, Rpc> LoadPendingBlock
    for BerachainApi<Provider, Pool, Network, EvmConfig, Rpc>
where
    Self: SpawnBlocking<
            NetworkTypes = Rpc::Network,
            Error: FromEvmError<Self::Evm>,
            RpcConvert: RpcConvert<Network = Rpc::Network>,
        > + RpcNodeCore<
            Provider: BlockReaderIdExt<Receipt = Provider::Receipt, Block = Provider::Block>
                          + ChainSpecProvider<ChainSpec: EthChainSpec + EthereumHardforks>
                          + StateProviderFactory,
            Pool: TransactionPool<
                Transaction: PoolTransaction<Consensus = ProviderTx<Self::Provider>>,
            >,
            Evm = EvmConfig,
            Primitives: NodePrimitives<
                BlockHeader = ProviderHeader<Self::Provider>,
                SignedTx = ProviderTx<Self::Provider>,
                Receipt = ProviderReceipt<Self::Provider>,
                Block = ProviderBlock<Self::Provider>,
            >,
        >,
    Provider: BlockReader,
    EvmConfig: ConfigureEvm<Primitives = Self::Primitives>,
    Rpc: RpcConvert<
        Network: RpcTypes<Header = alloy_rpc_types_eth::Header<ProviderHeader<Self::Provider>>>,
    >,
{
    #[inline]
    fn pending_block(
        &self,
    ) -> &tokio::sync::Mutex<
        Option<PendingBlock<ProviderBlock<Self::Provider>, ProviderReceipt<Self::Provider>>>,
    > {
        self.inner.pending_block()
    }

    #[inline]
    fn pending_env_builder(&self) -> &dyn PendingEnvBuilder<Self::Evm> {
        self.inner.pending_env_builder()
    }
}
