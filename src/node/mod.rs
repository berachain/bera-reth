//! Berachain node implementation using Reth's component-based architecture

pub mod evm;

use crate::{
    chainspec::BerachainChainSpec,
    consensus::BerachainConsensusBuilder,
    engine::{
        BerachainEngineTypes, builder::BerachainPayloadServiceBuilder,
        validator::BerachainEngineValidatorBuilder,
    },
    node::evm::BerachainExecutorBuilder,
    pool::BerachainPoolBuilder,
    primitives::{BerachainHeader, BerachainPrimitives},
    rpc::{BerachainAddOns, BerachainEthApiBuilder},
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::{SignableTransaction, error::ValueError};
use alloy_primitives::Signature;
use alloy_rpc_types::TransactionRequest;
use reth::{
    api::{BlockTy, FullNodeTypes, NodeTypes},
    providers::EthStorage,
    rpc::compat::TryIntoSimTx,
};
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_node_api::FullNodeComponents;
use reth_node_builder::{
    DebugNode, Node, NodeAdapter, NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder},
};
use reth_node_core::args::DefaultEngineValues;
use reth_node_ethereum::node::EthereumNetworkBuilder;
use reth_payload_primitives::{PayloadAttributesBuilder, PayloadTypes};
use std::sync::Arc;

/// Persist every canonical block to disk immediately rather than buffering.
/// Upstream reth defaults to 7, but Berachain's faster block times benefit from
/// eager persistence to keep the in-memory block window minimal.
const BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD: u64 = 0;

/// Keep zero recent blocks in memory by default, preserving pre-v2.5.0 behavior after
/// upstream raised `DEFAULT_MEMORY_BLOCK_BUFFER_TARGET` from 0 to 5 (paradigmxyz/reth#26462).
/// An explicit `--engine.memory-block-buffer-target` flag still overrides this.
const BERACHAIN_DEFAULT_MEMORY_BLOCK_BUFFER_TARGET: u64 = 0;

/// Installs Berachain-tuned engine CLI defaults. Must be called before CLI parsing.
pub fn init_engine_defaults() -> Result<(), DefaultEngineValues> {
    DefaultEngineValues::default()
        .with_persistence_threshold(BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD)
        .with_memory_block_buffer_target(BERACHAIN_DEFAULT_MEMORY_BLOCK_BUFFER_TARGET)
        .try_init()
}

/// Type configuration for a regular Berachain node.

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainNode;

impl NodeTypes for BerachainNode {
    type Primitives = BerachainPrimitives;
    type ChainSpec = BerachainChainSpec;
    type Storage = EthStorage<BerachainTxEnvelope, BerachainHeader>;
    type Payload = BerachainEngineTypes;
}

impl TryIntoSimTx<BerachainTxEnvelope> for TransactionRequest {
    fn try_into_sim_tx(self) -> Result<BerachainTxEnvelope, ValueError<Self>> {
        let tx = self
            .build_typed_tx()
            .map_err(|req| ValueError::new(req, "Transaction is not buildable"))?;
        let signature = Signature::new(Default::default(), Default::default(), false);
        Ok(tx.into_signed(signature).into())
    }
}

impl<N> Node<N> for BerachainNode
where
    N: FullNodeTypes<Types = Self>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        BerachainPoolBuilder,
        BasicPayloadServiceBuilder<BerachainPayloadServiceBuilder>,
        EthereumNetworkBuilder,
        BerachainExecutorBuilder,
        BerachainConsensusBuilder,
    >;

    type AddOns = BerachainAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
        BerachainEthApiBuilder,
        BerachainEngineValidatorBuilder,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        ComponentsBuilder::default()
            .node_types()
            .pool(BerachainPoolBuilder)
            .executor(BerachainExecutorBuilder)
            .payload(BasicPayloadServiceBuilder::new(BerachainPayloadServiceBuilder::default()))
            .network(EthereumNetworkBuilder::default())
            .consensus(BerachainConsensusBuilder)
    }

    fn add_ons(&self) -> Self::AddOns {
        BerachainAddOns::default()
    }
}

impl<N> DebugNode<N> for BerachainNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block<BerachainTxEnvelope, BerachainHeader>;

    fn rpc_to_primitive_block(rpc_block: Self::RpcBlock) -> BlockTy<Self> {
        rpc_block.into_consensus_block().convert_transactions()
    }

    fn local_payload_attributes_builder(
        chain_spec: &Self::ChainSpec,
    ) -> impl PayloadAttributesBuilder<
        <<Self as NodeTypes>::Payload as PayloadTypes>::PayloadAttributes,
        BerachainHeader,
    > {
        LocalPayloadAttributesBuilder::new(Arc::new(chain_spec.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use reth_node_core::args::EngineArgs;

    #[derive(Parser)]
    struct TestParser {
        #[command(flatten)]
        args: EngineArgs,
    }

    /// Single test covering all default assertions: the engine-defaults global can only be
    /// initialized once per process.
    #[test]
    fn engine_defaults_pin_zero_memory_block_buffer_target() {
        init_engine_defaults().expect("engine defaults must initialize once");

        let parsed = TestParser::parse_from(["bera-reth"]);
        assert_eq!(parsed.args.persistence_threshold, 0);
        assert_eq!(parsed.args.memory_block_buffer_target(), 0);
        let tree = parsed.args.tree_config();
        assert_eq!(tree.persistence_threshold(), 0);
        assert_eq!(tree.memory_block_buffer_target(), 0);

        // A raised persistence threshold must not silently re-enable in-memory buffering
        // via upstream's `min(persistence_threshold, default)` fallback.
        let parsed = TestParser::parse_from(["bera-reth", "--engine.persistence-threshold", "7"]);
        assert_eq!(parsed.args.memory_block_buffer_target(), 0);
        assert_eq!(parsed.args.tree_config().memory_block_buffer_target(), 0);

        // An explicit flag still wins over the pinned default.
        let parsed =
            TestParser::parse_from(["bera-reth", "--engine.memory-block-buffer-target", "3"]);
        assert_eq!(parsed.args.memory_block_buffer_target(), 3);
        assert_eq!(parsed.args.tree_config().memory_block_buffer_target(), 3);
    }
}
