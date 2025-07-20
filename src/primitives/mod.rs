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
    type Block = BerachainBlock; // Uses your transaction type
    type BlockHeader = BerachainHeader; // Custom Berachain header with prev_proposer_pubkey
    type BlockBody = BerachainBlockBody; // Uses your transaction type
    type SignedTx = BerachainTxEnvelope; // Your custom transaction envelope
    type Receipt = reth_ethereum_primitives::Receipt<BerachainTxType>; // Berachain receipts with transaction type
}
