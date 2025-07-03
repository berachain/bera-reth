use crate::transaction::BerachainTxEnvelope;
use reth_primitives_traits::NodePrimitives;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct BerachainPrimitives;

pub type BerachainBlock = alloy_consensus::Block<BerachainTxEnvelope, alloy_consensus::Header>;

/// The body type of this node
pub type BerachainBlockBody =
    alloy_consensus::BlockBody<BerachainTxEnvelope, alloy_consensus::Header>;

impl NodePrimitives for BerachainPrimitives {
    type Block = BerachainBlock; // Uses your transaction type
    type BlockHeader = alloy_consensus::Header; // Standard Ethereum header
    type BlockBody = BerachainBlockBody; // Uses your transaction type
    type SignedTx = BerachainTxEnvelope; // Your custom transaction envelope
    type Receipt = reth_ethereum_primitives::Receipt; // Standard Ethereum receipts
}
