pub mod transaction;
mod validate;

use crate::{
    chainspec::BerachainChainSpec,
    pool::{
        transaction::BerachainPooledTransaction,
        validate::LegacyMinimumPriorityFeeValidator,
    },
    primitives::BerachainPrimitives,
};
use alloy_eips::{eip7840::BlobParams, merge::EPOCH_SLOTS};
use reth::{api::NodeTypes, transaction_pool::blobstore::DiskFileBlobStore};
use reth_chainspec::EthChainSpec;
use reth_evm::ConfigureEvm;
use reth_node_api::FullNodeTypes;
use reth_node_builder::{
    BuilderContext,
    components::{PoolBuilder, TxPoolBuilder},
};
use reth_transaction_pool::{
    CoinbaseTipOrdering, EthTransactionValidator, Pool, TransactionValidationTaskExecutor,
};
use std::{time::SystemTime};
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
            LegacyMinimumPriorityFeeValidator<
                EthTransactionValidator<Node::Provider, BerachainPooledTransaction, Evm>,
                Node::Provider,
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

        let minimum_priority_fee = ctx.config().txpool.minimum_priority_fee;
        let local_transactions_config = pool_config.local_transactions_config.clone();
        let provider = ctx.provider().clone();

        let eth_validator = TransactionValidationTaskExecutor::eth_builder(provider.clone(), evm_config)
            .set_eip4844(!blobs_disabled)
            // BRIP-0010 Osaka does not adopt EIP-7594 (PeerDAS); keep EIP-4844 sidecars
            // accepted across forks and reject EIP-7594 v1 sidecars.
            .no_eip7594()
            .with_max_tx_input_bytes(ctx.config().txpool.max_tx_input_bytes)
            .kzg_settings(ctx.kzg_settings()?)
            .with_local_transactions_config(local_transactions_config.clone())
            .set_tx_fee_cap(ctx.config().rpc.rpc_tx_fee_cap)
            .with_max_tx_gas_limit(ctx.config().txpool.max_tx_gas_limit)
            .with_minimum_priority_fee(minimum_priority_fee)
            .with_additional_tasks(ctx.config().txpool.additional_validation_tasks)
            .build_with_tasks(ctx.task_executor().clone(), blob_store.clone());

        if eth_validator.validator().eip4844() {
            let kzg_settings = eth_validator.validator().kzg_settings().clone();
            ctx.task_executor().spawn_blocking_task(async move {
                let _ = kzg_settings.get();
                debug!(target: "reth::cli", "Initialized KZG settings");
            });
        }

        let validator = eth_validator.map(move |inner| {
            LegacyMinimumPriorityFeeValidator::new(
                inner,
                provider.clone(),
                minimum_priority_fee,
                local_transactions_config.clone(),
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
