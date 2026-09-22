pub mod min_priority_fee;
pub mod config;
pub mod transaction;

use crate::{
    chainspec::BerachainChainSpec,
    pool::{min_priority_fee::MinPriorityFeeValidator, transaction::BerachainPooledTransaction, config::BERACHAIN_ACCEPTS_EIP7594,},
    primitives::BerachainPrimitives,
};
use alloy_consensus::BlockHeader;
use alloy_eips::{eip7840::BlobParams, merge::EPOCH_SLOTS};
use reth::{api::NodeTypes, transaction_pool::blobstore::DiskFileBlobStore};
use reth_chainspec::EthChainSpec;
use reth_evm::ConfigureEvm;
use reth_node_api::FullNodeTypes;
use reth_node_builder::{
    BuilderContext,
    components::{PoolBuilder, TxPoolBuilder},
};
use reth_storage_api::BlockReaderIdExt;
use reth_transaction_pool::{
    CoinbaseTipOrdering, EthTransactionValidator, Pool, TransactionValidationTaskExecutor,
};
use std::{fmt::Debug, time::SystemTime};
use tracing::{debug, info};

#[derive(Debug, Default)]
pub struct BerachainPoolBuilder;

impl<Types, Node, Evm> PoolBuilder<Node, Evm> for BerachainPoolBuilder
where
    Types: NodeTypes<ChainSpec = BerachainChainSpec, Primitives = BerachainPrimitives>,
    Node: FullNodeTypes<Types = Types>,
    Evm: ConfigureEvm<Primitives = BerachainPrimitives> + Clone + 'static,
{
    type Pool = Pool<
        TransactionValidationTaskExecutor<
            MinPriorityFeeValidator<
                EthTransactionValidator<Node::Provider, BerachainPooledTransaction, Evm>,
                BerachainChainSpec,
            >,
        >,
        CoinbaseTipOrdering<BerachainPooledTransaction>,
        DiskFileBlobStore,
    >;

    async fn build_pool(
        self,
        ctx: &BuilderContext<Node>,
        evm_config: Evm,
    ) -> eyre::Result<Self::Pool> {
        let pool_config = ctx.pool_config();

        let blobs_disabled = ctx.config().txpool.disable_blobs_support ||
            ctx.config().txpool.blobpool_max_count == 0;

        let blob_cache_size = if let Some(blob_cache_size) = pool_config.blob_cache_size {
            Some(blob_cache_size)
        } else {
            let current_timestamp =
                SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs();
            let blob_params = ctx
                .chain_spec()
                .blob_params_at_timestamp(current_timestamp)
                .unwrap_or_else(BlobParams::cancun);

            Some((blob_params.target_blob_count * EPOCH_SLOTS * 2) as u32)
        };

        let blob_store =
            reth_node_builder::components::create_blob_store_with_cache(ctx, blob_cache_size)?;

        let validator =
            TransactionValidationTaskExecutor::eth_builder(ctx.provider().clone(), evm_config)
                .set_eip4844(!blobs_disabled)
                .set_eip7594(BERACHAIN_ACCEPTS_EIP7594)
                .with_max_tx_input_bytes(ctx.config().txpool.max_tx_input_bytes)
                .kzg_settings(ctx.kzg_settings()?)
                .with_local_transactions_config(pool_config.local_transactions_config.clone())
                .set_tx_fee_cap(ctx.config().rpc.rpc_tx_fee_cap)
                .with_max_tx_gas_limit(ctx.config().txpool.max_tx_gas_limit)
                .with_additional_tasks(ctx.config().txpool.additional_validation_tasks)
                .build_with_tasks(ctx.task_executor().clone(), blob_store.clone());

        if validator.validator().eip4844() {
            let kzg_settings = validator.validator().kzg_settings().clone();
            ctx.task_executor().spawn_blocking_task(async move {
                let _ = kzg_settings.get();
                debug!(target: "reth::cli", "Initialized KZG settings");
            });
        }

        // Enforce the configured minimum priority fee across ALL transaction types. reth's
        // built-in filter exempts legacy/EIP-2930 and checks the declared fee cap instead of
        // the effective tip, both of which a spammer can exploit.
        let minimum_priority_fee = ctx.config().txpool.minimum_priority_fee.unwrap_or(0);
        let chain_spec = ctx.chain_spec();
        let initial_base_fee = ctx
            .provider()
            .latest_header()?
            .and_then(|header| chain_spec.next_block_base_fee(header.header(), header.timestamp()))
            .unwrap_or_default();
        let local_transactions_config = pool_config.local_transactions_config.clone();
        let validator = validator.map(move |inner| {
            MinPriorityFeeValidator::new(
                inner,
                minimum_priority_fee,
                local_transactions_config.clone(),
                chain_spec.clone(),
                initial_base_fee,
            )
        });

        let transaction_pool = TxPoolBuilder::new(ctx)
            .with_validator(validator)
            .build_and_spawn_maintenance_task(blob_store, pool_config)?;

        info!(target: "reth::cli", "Transaction pool initialized");
        debug!(target: "reth::cli", "Spawned txpool maintenance task");

        Ok(transaction_pool)
    }
}
