use reth::{
    api::FullNodeComponents,
    rpc::eth::{EthApiFor, FullEthApiServer},
};
use reth_node_builder::rpc::{EthApiBuilder, EthApiCtx};

pub mod engine_api;

#[derive(Debug, Default)]
#[non_exhaustive]
pub struct BerachainApiBuilder;

impl<N> EthApiBuilder<N> for BerachainApiBuilder
where
    N: FullNodeComponents,
    EthApiFor<N>: FullEthApiServer<Provider = N::Provider, Pool = N::Pool>,
{
    type EthApi = EthApiFor<N>;

    async fn build_eth_api(self, ctx: EthApiCtx<'_, N>) -> eyre::Result<Self::EthApi> {
        let api = reth_rpc::EthApiBuilder::new(
            ctx.components.provider().clone(),
            ctx.components.pool().clone(),
            ctx.components.network().clone(),
            ctx.components.evm_config().clone(),
        )
        .eth_cache(ctx.cache)
        .task_spawner(ctx.components.task_executor().clone())
        .gas_cap(ctx.config.rpc_gas_cap.into())
        .max_simulate_blocks(ctx.config.rpc_max_simulate_blocks)
        .eth_proof_window(ctx.config.eth_proof_window)
        .fee_history_cache_config(ctx.config.fee_history_cache)
        .proof_permits(ctx.config.proof_permits)
        .gas_oracle_config(ctx.config.gas_oracle)
        .build();
        Ok(api)
    }
}
