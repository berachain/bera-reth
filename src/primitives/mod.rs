use crate::transaction::{BerachainTxEnvelope, BerachainTxType};
use reth_primitives_traits::NodePrimitives;

pub mod header;
pub use header::BerachainHeader;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct BerachainPrimitives;

pub type BerachainBlock = alloy_consensus::Block<BerachainTxEnvelope, BerachainHeader>;

/// The body type of this node
pub type BerachainBlockBody = alloy_consensus::BlockBody<BerachainTxEnvelope, BerachainHeader>;

impl NodePrimitives for BerachainPrimitives {
    type Block = BerachainBlock;
    type BlockHeader = BerachainHeader;
    type BlockBody = BerachainBlockBody;
    type SignedTx = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt<BerachainTxType>;
}
