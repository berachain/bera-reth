use crate::{
    flashblocks::{BerachainFlashblockPayload, FlashblocksListeners},
    primitives::BerachainHeader,
    rpc::{receipt::BerachainReceiptEnvelope, record_state_fallback, record_state_lookup},
    transaction::{BerachainTxEnvelope, BerachainTxType, POL_TX_TYPE},
};
use alloy_consensus::{BlockHeader, Transaction};
use alloy_eips::{BlockId, BlockNumberOrTag, eip2930::AccessList};
use alloy_network::{
    BuildResult, Network, NetworkWallet, TransactionBuilder, TransactionBuilderError,
};
use alloy_primitives::{Address, B256, Bytes, ChainId, TxKind, U256};
use alloy_rpc_types_eth::{Transaction as RpcTransaction, TransactionRequest};
use core::fmt;
use derive_more::Deref;
use reth::{
    providers::{BlockReaderIdExt, ProviderHeader},
    rpc::compat::RpcConvert,
    tasks::{
        TaskSpawner,
        pool::{BlockingTaskGuard, BlockingTaskPool},
    },
};
use reth_chain_state::BlockState;
use reth_primitives_traits::{BlockBody, RecoveredBlock};
use reth_rpc_eth_api::{
    EthApiTypes, FromEthApiError, RpcNodeCore, RpcNodeCoreExt, RpcReceipt,
    helpers::{
        Call, EthApiSpec, EthBlocks, EthCall, EthFees, EthState, EthTransactions, LoadBlock,
        LoadFee, LoadPendingBlock, LoadReceipt, LoadState, LoadTransaction, SpawnBlocking, Trace,
        estimate::EstimateCall, pending_block::PendingEnvBuilder, spec::SignersForRpc,
    },
};
use reth_rpc_eth_types::{
    EthApiError, EthStateCache, FeeHistoryCache, GasPriceOracle, PendingBlock,
    block::BlockAndReceipts, builder::config::PendingBlockKind, error::FromEvmError,
};
use reth_storage_api::{BlockIdReader, BlockReader, StateProviderBox, StateProviderFactory};
use reth_transaction_pool::{
    AddedTransactionOutcome, PoolTransaction, TransactionOrigin, TransactionPool,
};
use std::sync::Arc;

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
pub struct BerachainApi<N: RpcNodeCore, Rpc: RpcConvert> {
    /// All nested fields bundled together.
    #[deref]
    pub(super) inner: reth_rpc::EthApi<N, Rpc>,

    /// Flashblocks listeners.
    ///
    /// If set, provides receivers for pending blocks, flashblock sequences, and build status.
    pub flashblocks: Option<Arc<FlashblocksListeners<N::Primitives, BerachainFlashblockPayload>>>,
}

impl<N: RpcNodeCore, Rpc: RpcConvert> BerachainApi<N, Rpc>
where
    N::Provider: BlockReaderIdExt,
{
    /// Returns the current pending block from the flashblock stream, if any.
    ///
    /// Only returns a block whose parent hash matches the latest canonical header,
    /// ensuring stale flashblocks are not served during reorgs.
    pub fn pending_flashblock(&self) -> Option<PendingBlock<N::Primitives>> {
        let latest_hash = match self.provider().latest_header().ok().flatten() {
            Some(h) => h.hash(),
            None => {
                record_state_lookup("miss_no_header");
                return None;
            }
        };

        let Some(flashblocks) = self.flashblocks.as_ref() else {
            record_state_lookup("miss_no_listener");
            return None;
        };

        let pending = flashblocks.pending_block_rx.borrow().as_ref().map(|b| b.pending.clone());

        match pending {
            None => {
                record_state_lookup("miss_no_pending");
                None
            }
            Some(p) if p.block().parent_hash() == latest_hash => {
                record_state_lookup("hit");
                Some(p)
            }
            Some(_) => {
                record_state_lookup("miss_stale_hash");
                None
            }
        }
    }
}

impl<N, Rpc> Clone for BerachainApi<N, Rpc>
where
    N: RpcNodeCore,
    Rpc: RpcConvert,
{
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone(), flashblocks: self.flashblocks.clone() }
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

    async fn send_transaction(
        &self,
        origin: TransactionOrigin,
        tx: reth_primitives_traits::WithEncoded<
            reth_primitives_traits::Recovered<reth_transaction_pool::PoolPooledTx<Self::Pool>>,
        >,
    ) -> Result<B256, Self::Error> {
        let (raw_tx, recovered) = tx.split();

        self.broadcast_raw_transaction(raw_tx);

        let pool_transaction = <Self::Pool as TransactionPool>::Transaction::from_pooled(recovered);

        let AddedTransactionOutcome { hash, .. } =
            self.pool().add_transaction(origin, pool_transaction).await?;

        Ok(hash)
    }

    fn transaction_receipt(
        &self,
        hash: B256,
    ) -> impl Future<Output = Result<Option<RpcReceipt<Self::NetworkTypes>>, Self::Error>> + Send
    {
        let this = self.clone();
        async move {
            let tx_receipt = this.load_transaction_and_receipt(hash).await?;

            if tx_receipt.is_none() &&
                let Some(pending_block) = this.pending_flashblock() &&
                let Some(Ok(receipt)) =
                    pending_block.find_and_convert_transaction_receipt(hash, this.converter())
            {
                return Ok(Some(receipt));
            }
            let Some((tx, meta, receipt)) = tx_receipt else { return Ok(None) };
            this.build_transaction_receipt(tx, meta, receipt).await.map(Some)
        }
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
    #[allow(clippy::manual_async_fn)]
    fn block_transaction_count(
        &self,
        block_id: BlockId,
    ) -> impl Future<Output = Result<Option<usize>, Self::Error>> + Send {
        async move {
            if (block_id.is_latest() || block_id.is_pending()) &&
                let Some(pending) = self.pending_flashblock()
            {
                return Ok(Some(pending.block().body().transaction_count()));
            }

            if block_id.is_pending() {
                return Ok(self
                    .provider()
                    .pending_block()
                    .map_err(Into::<EthApiError>::into)?
                    .map(|block| block.body().transaction_count()));
            }

            let block_hash = match self
                .provider()
                .block_hash_for_id(block_id)
                .map_err(Into::<EthApiError>::into)?
            {
                Some(block_hash) => block_hash,
                None => return Ok(None),
            };

            Ok(self
                .cache()
                .get_recovered_block(block_hash)
                .await
                .map_err(Into::<EthApiError>::into)?
                .map(|b| b.body().transaction_count()))
        }
    }
}

impl<N, Rpc> LoadBlock for BerachainApi<N, Rpc>
where
    Self: LoadPendingBlock,
    N: RpcNodeCore,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError>,
{
    #[allow(clippy::manual_async_fn, clippy::collapsible_if)]
    fn recovered_block(
        &self,
        block_id: BlockId,
    ) -> impl Future<
        Output = Result<
            Option<Arc<RecoveredBlock<<Self::Provider as BlockReader>::Block>>>,
            Self::Error,
        >,
    > + Send {
        async move {
            // Serve flashblock for both "latest" and "pending" when available
            if block_id.is_latest() || block_id.is_pending() {
                if self.pending_flashblock().is_some() {
                    if let Some(pending) = self.local_pending_block().await? {
                        return Ok(Some(pending.block));
                    }
                }
            }

            // Default pending fallback: CL-provided block, then locally built
            if block_id.is_pending() {
                if let Some(pending_block) =
                    self.provider().pending_block().map_err(Self::Error::from_eth_err)?
                {
                    return Ok(Some(Arc::new(pending_block)));
                }

                return match self.local_pending_block().await? {
                    Some(pending) => Ok(Some(pending.block)),
                    None => Ok(None),
                };
            }

            let block_hash = match self
                .provider()
                .block_hash_for_id(block_id)
                .map_err(Self::Error::from_eth_err)?
            {
                Some(block_hash) => block_hash,
                None => return Ok(None),
            };

            self.cache().get_recovered_block(block_hash).await.map_err(Self::Error::from_eth_err)
        }
    }
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
    #[allow(clippy::manual_async_fn, clippy::collapsible_if)]
    fn state_at_block_id_or_latest(
        &self,
        block_id: Option<BlockId>,
    ) -> impl Future<Output = Result<StateProviderBox, Self::Error>> + Send
    where
        Self: SpawnBlocking,
    {
        async move {
            let should_use_flashblock = block_id.is_none_or(|id| id.is_latest() || id.is_pending());

            if should_use_flashblock {
                if let Ok(Some(state)) = self.local_pending_state().await {
                    return Ok(state);
                }
            }

            if let Some(block_id) = block_id {
                self.state_at_block_id(block_id).await
            } else {
                Ok(self.latest_state()?)
            }
        }
    }
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

    #[allow(clippy::manual_async_fn)]
    fn gas_price(&self) -> impl Future<Output = Result<U256, Self::Error>> + Send {
        async move {
            let suggested_tip = LoadFee::suggested_priority_fee(self).await?;

            let base_fee = if let Some(pending) = self.pending_flashblock() {
                pending.block().base_fee_per_gas().unwrap_or_default()
            } else {
                self.provider()
                    .latest_header()
                    .map_err(Into::<EthApiError>::into)?
                    .and_then(|h| h.base_fee_per_gas())
                    .unwrap_or_default()
            };

            Ok(suggested_tip + U256::from(base_fee))
        }
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

    /// Returns the pending state built on top of the latest flashblock.
    ///
    /// If the flashblock's parent block hasn't been imported into the DB yet, falls back to
    /// canonical state via `Ok(None)`.
    async fn local_pending_state(&self) -> Result<Option<StateProviderBox>, Self::Error>
    where
        Self: SpawnBlocking,
    {
        let Some(pending_block) = self.pending_flashblock() else {
            tracing::info!("no pending flashblock available, falling back to canonical state");
            return Ok(None);
        };

        let parent_hash = pending_block.block().parent_hash();

        let Ok(latest_historical) =
            self.provider().history_by_block_hash(parent_hash).map_err(Into::<EthApiError>::into)
        else {
            record_state_fallback("parent_absent");
            tracing::info!(
                %parent_hash,
                "parent block not imported yet, falling back to canonical state"
            );
            return Ok(None);
        };

        let state = BlockState::from(pending_block);
        Ok(Some(Box::new(state.state_provider(latest_historical)) as StateProviderBox))
    }

    async fn local_pending_block(
        &self,
    ) -> Result<Option<BlockAndReceipts<Self::Primitives>>, Self::Error> {
        if let Some(pending) = self.pending_flashblock() {
            return Ok(Some(pending.into_block_and_receipts()));
        }

        let latest = self
            .provider()
            .latest_header()?
            .ok_or(EthApiError::HeaderNotFound(BlockNumberOrTag::Latest.into()))?;

        let latest = self
            .cache()
            .get_block_and_receipts(latest.hash())
            .await
            .map_err(Into::<EthApiError>::into)?
            .map(|(block, receipts)| BlockAndReceipts { block, receipts });
        Ok(latest)
    }
}
