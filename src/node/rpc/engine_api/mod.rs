use crate::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth::api::{AddOnsContext, FullNodeComponents, NodeTypes};
use reth_node_builder::rpc::EngineValidatorBuilder;
use reth_node_ethereum::EthereumEngineValidator;

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainEngineValidatorBuilder;

impl<Node, Types> EngineValidatorBuilder<Node> for BerachainEngineValidatorBuilder
where
    Types: NodeTypes<
            ChainSpec = <BerachainNode as NodeTypes>::ChainSpec,
            Primitives = <BerachainNode as NodeTypes>::Primitives,
            Payload = <BerachainNode as NodeTypes>::Payload,
        >,
    Node: FullNodeComponents<Types = Types>,
{
    type Validator = EthereumEngineValidator<BerachainChainSpec>;

    async fn build(self, ctx: &AddOnsContext<'_, Node>) -> eyre::Result<Self::Validator> {
        Ok(EthereumEngineValidator::new(ctx.config.chain.clone()))
    }
}
