use crate::{
    primitives::BerachainHeader,
    rpc::receipt::BerachainReceiptEnvelope,
    transaction::{BerachainTxEnvelope, BerachainTxType, POL_TX_TYPE},
};
use alloy_consensus::Transaction;
use alloy_primitives::{B256, U256};
use alloy_rpc_types_eth::{Transaction as RpcTransaction, TransactionRequest};
use core::fmt;
use derive_more::Deref;
use reth::{
    providers::ProviderHeader,
    rpc::compat::RpcConvert,
    tasks::{
        Runtime,
        pool::{BlockingTaskGuard, BlockingTaskPool},
    },
    transaction_pool::TransactionPool,
};
use reth_rpc_eth_api::{
    EthApiTypes, RpcNodeCore, RpcNodeCoreExt,
    helpers::{
        Call, EthApiSpec, EthBlocks, EthCall, EthFees, EthState, EthSubscriptions, EthTransactions,
        LoadBlock, LoadFee, LoadPendingBlock, LoadReceipt, LoadState, LoadTransaction,
        SpawnBlocking, Trace, bal::GetBlockAccessList, estimate::EstimateCall,
        pending_block::PendingEnvBuilder, spec::SignersForRpc,
    },
};
use reth_rpc_eth_types::{
    EthApiError, EthStateCache, FeeHistoryCache, GasPriceOracle, PendingBlock,
    builder::config::PendingBlockKind, error::FromEvmError,
};
use reth_transaction_pool::{AddedTransactionOutcome, TransactionOrigin};

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

#[derive(Clone, Copy, Debug)]
pub struct BerachainNetwork {
    _private: (),
}

impl reth_rpc_convert::RpcTypes for BerachainNetwork {
    type Header = alloy_rpc_types_eth::Header<BerachainHeader>;
    type Receipt = alloy_rpc_types_eth::TransactionReceipt<BerachainReceiptEnvelope>;
    type TransactionResponse = RpcTransaction<BerachainTxEnvelope>;
    type TransactionRequest = TransactionRequest;
    type Log = alloy_rpc_types_eth::Log;
}

#[derive(Deref)]
pub struct BerachainApi<N: RpcNodeCore, Rpc: RpcConvert> {
    /// All nested fields bundled together.
    #[deref]
    pub(super) inner: reth_rpc::EthApi<N, Rpc>,
}

impl<N, Rpc> Clone for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert,
{
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<N, Rpc> EthApiTypes for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert<Error = EthApiError>,
{
    type Error = EthApiError;

    type NetworkTypes = Rpc::Network;
    type RpcConvert = Rpc;

    fn converter(&self) -> &Self::RpcConvert {
        self.inner.converter()
    }
}

impl<N, Rpc> RpcNodeCore for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert,
{
    type Primitives = N::Primitives;
    type Provider = N::Provider;
    type Pool = N::Pool;
    type Evm = N::Evm;
    type Network = N::Network;

    fn pool(&self) -> &Self::Pool {
        self.inner.pool()
    }

    fn evm_config(&self) -> &Self::Evm {
        self.inner.evm_config()
    }

    fn network(&self) -> &Self::Network {
        self.inner.network()
    }

    fn provider(&self) -> &Self::Provider {
        self.inner.provider()
    }
}

impl<N, Rpc> RpcNodeCoreExt for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert,
{
    #[inline]
    fn cache(&self) -> &EthStateCache<N::Primitives> {
        self.inner.cache()
    }
}

impl<N, Rpc> std::fmt::Debug for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EthApi").finish_non_exhaustive()
    }
}

impl<N, Rpc> SpawnBlocking for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert<Error = EthApiError>,
{
    #[inline]
    fn io_task_spawner(&self) -> &Runtime {
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

    #[inline]
    fn blocking_io_task_guard(&self) -> &std::sync::Arc<tokio::sync::Semaphore> {
        self.inner.blocking_io_request_semaphore()
    }
}

impl<N, Rpc> EthTransactions for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
    #[inline]
    fn signers(&self) -> &SignersForRpc<Self::Provider, Self::NetworkTypes> {
        EthTransactions::signers(&self.inner)
    }

    fn send_raw_transaction_sync_timeout(&self) -> std::time::Duration {
        EthTransactions::send_raw_transaction_sync_timeout(&self.inner)
    }

    async fn send_pool_transaction(
        &self,
        origin: TransactionOrigin,
        tx: reth_primitives_traits::WithEncoded<<Self::Pool as TransactionPool>::Transaction>,
    ) -> Result<B256, Self::Error> {
        let (raw_tx, pool_transaction) = tx.split();

        // broadcast raw transaction to subscribers if there is any.
        self.broadcast_raw_transaction(raw_tx);

        let AddedTransactionOutcome { hash, .. } =
            self.pool().add_transaction(origin, pool_transaction).await?;

        Ok(hash)
    }
}

impl<N, Rpc> LoadTransaction for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
}

impl<N, Rpc> LoadReceipt for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
}

impl<N, Rpc> GetBlockAccessList for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError, Evm = N::Evm>,
{
}

impl<N, Rpc> EthSubscriptions for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
}

impl<N, Rpc> EthApiSpec for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
    fn starting_block(&self) -> U256 {
        self.inner.starting_block()
    }
}

impl<N, Rpc> EthBlocks for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
}

impl<N, Rpc> LoadBlock for BerachainApi<N, Rpc>
where
    Self: LoadPendingBlock,
    N: RpcNodeCore,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
}

impl<N, Rpc> EthCall for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Evm = N::Evm, Error = EthApiError>,
{
}

impl<N, Rpc> Call for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Evm = N::Evm, Error = EthApiError>,
{
    #[inline]
    fn call_gas_limit(&self) -> u64 {
        self.inner.gas_cap()
    }

    #[inline]
    fn max_simulate_blocks(&self) -> u64 {
        self.inner.max_simulate_blocks()
    }

    #[inline]
    fn compute_state_root_for_eth_simulate(&self) -> bool {
        Call::compute_state_root_for_eth_simulate(&self.inner)
    }

    #[inline]
    fn evm_memory_limit(&self) -> u64 {
        self.inner.evm_memory_limit()
    }
}

impl<N, Rpc> EstimateCall for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Evm = N::Evm, Error = EthApiError>,
{
}

impl<N, Rpc> EthFees for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Evm = N::Evm, Error = EthApiError>,
{
}

impl<N, Rpc> EthState for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
    Self: LoadPendingBlock,
{
    fn max_proof_window(&self) -> u64 {
        self.inner.eth_proof_window()
    }
}

impl<N, Rpc> Trace for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Evm = N::Evm, Error = EthApiError>,
{
}

impl<N, Rpc> LoadState for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
    Self: LoadPendingBlock,
{
}

impl<N, Rpc> LoadFee for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Evm = N::Evm, Error = EthApiError>,
{
    #[inline]
    fn gas_oracle(&self) -> &GasPriceOracle<Self::Provider> {
        self.inner.gas_oracle()
    }

    #[inline]
    fn fee_history_cache(&self) -> &FeeHistoryCache<ProviderHeader<N::Provider>> {
        self.inner.fee_history_cache()
    }
}

impl<N, Rpc> LoadPendingBlock for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
    #[inline]
    fn pending_block(&self) -> &tokio::sync::Mutex<Option<PendingBlock<Self::Primitives>>> {
        self.inner.pending_block()
    }

    #[inline]
    fn pending_env_builder(&self) -> &dyn PendingEnvBuilder<Self::Evm> {
        self.inner.pending_env_builder()
    }

    #[inline]
    fn pending_block_kind(&self) -> PendingBlockKind {
        self.inner.pending_block_kind()
    }
}
