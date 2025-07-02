use crate::transaction::BerachainTxEnvelope;
use reth_primitives_traits::NodePrimitives;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BerachainPrimitives;

pub type Block = alloy_consensus::Block<BerachainTxEnvelope, alloy_consensus::Header>;

/// The body type of this node
pub type BlockBody = alloy_consensus::BlockBody<BerachainTxEnvelope, alloy_consensus::Header>;

impl NodePrimitives for BerachainPrimitives {
    type Block = Block; // Uses your transaction type
    type BlockHeader = alloy_consensus::Header; // Standard Ethereum header
    type BlockBody = BlockBody; // Uses your transaction type
    type SignedTx = BerachainTxEnvelope; // Your custom transaction envelope
    type Receipt = reth_ethereum_primitives::Receipt; // Standard Ethereum receipts
}
