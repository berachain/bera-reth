// use crate::{node::BerachainNode, node::evm::BerachainNextBlockEnvAttributes};
// use reth_chainspec::EthChainSpec;
// use reth_ethereum_payload_builder::EthereumBuilderConfig;
// use reth_evm::ConfigureEvm;
// use reth_node_api::FullNodeTypes;
// use reth_node_builder::{BuilderContext, PayloadBuilderConfig, components::PayloadBuilderBuilder};
// use reth_node_types::{PrimitivesTy, TxTy};
// use reth_transaction_pool::{PoolTransaction, TransactionPool};
//
// pub struct BerachainPayloadBuilder;
//
// impl<Node, Pool, Evm> PayloadBuilderBuilder<Node, Pool, Evm> for BerachainPayloadBuilder
// where
//     Node: FullNodeTypes<Types = BerachainNode>,
//     Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TxTy<Node::Types>>>
//         + Unpin
//         + 'static,
//     Evm: ConfigureEvm<
//             Primitives = PrimitivesTy<BerachainNode>,
//             NextBlockEnvCtx = BerachainNextBlockEnvAttributes,
//         > + 'static,
// {
//     type PayloadBuilder =
//         reth_ethereum_payload_builder::EthereumPayloadBuilder<Pool, Node::Provider, Evm>;
//
//     async fn build_payload_builder(
//         self,
//         ctx: &BuilderContext<Node>,
//         pool: Pool,
//         evm_config: Evm,
//     ) -> eyre::Result<Self::PayloadBuilder> {
//         let conf = ctx.payload_builder_config();
//         let chain = ctx.chain_spec().chain();
//         let gas_limit = conf.gas_limit_for(chain);
//
//         Ok(reth_ethereum_payload_builder::EthereumPayloadBuilder::new(
//             ctx.provider().clone(),
//             pool,
//             evm_config,
//             EthereumBuilderConfig::new().with_gas_limit(gas_limit),
//         ))
//     }
// }
