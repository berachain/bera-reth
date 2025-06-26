//! Berachain payload builder implementation

use crate::chainspec::BerachainChainSpec;
use reth_basic_payload_builder::{BasicPayloadJobGenerator, BasicPayloadJobGeneratorConfig};
use reth_node_builder::{BuilderContext, FullNodeTypes, PayloadServiceBuilder};
use reth_payload_builder::{PayloadBuilderHandle, PayloadBuilderService};
use reth_provider::CanonStateSubscriptions;

/// Berachain payload builder configuration
#[derive(Debug, Clone)]
pub struct BerachainPayloadBuilderConfig {
    /// Max time for payload building
    pub deadline: Option<std::time::Duration>,
    /// Maximum number of cached payloads
    pub max_payload_tasks: usize,
}

impl Default for BerachainPayloadBuilderConfig {
    fn default() -> Self {
        Self { deadline: Some(std::time::Duration::from_secs(12)), max_payload_tasks: 3 }
    }
}

/// Berachain payload service builder
#[derive(Debug, Default, Clone)]
pub struct BerachainPayloadBuilder {
    config: BerachainPayloadBuilderConfig,
}

impl BerachainPayloadBuilder {
    pub fn new(config: BerachainPayloadBuilderConfig) -> Self {
        Self { config }
    }
}

impl<Node, Pool> PayloadServiceBuilder<Node, Pool> for BerachainPayloadBuilder
where
    Node: FullNodeTypes,
    Node::Types: NodeTypes<ChainSpec = BerachainChainSpec>,
    Pool: Clone + Unpin + 'static,
{
    async fn spawn_payload_service(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
    ) -> eyre::Result<PayloadBuilderHandle<Node::Types>> {
        let conf = BasicPayloadJobGeneratorConfig::default()
            .interval(std::time::Duration::from_secs(1))
            .deadline(self.config.deadline.unwrap_or(std::time::Duration::from_secs(12)))
            .max_payload_tasks(self.config.max_payload_tasks);

        let payload_generator = BasicPayloadJobGenerator::with_builder(
            ctx.provider().clone(),
            pool,
            ctx.task_executor().clone(),
            conf,
            ctx.chain_spec().clone(),
        );

        let (payload_service, payload_builder) =
            PayloadBuilderService::new(payload_generator, ctx.provider().canonical_state_stream());

        ctx.task_executor().spawn_critical("payload builder service", Box::pin(payload_service));

        Ok(payload_builder)
    }
}
