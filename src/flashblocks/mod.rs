#[cfg(test)]
pub mod test_utils;

pub mod traits;
pub use traits::{FlashblockDiff, FlashblockPayload, FlashblockPayloadBase};

pub mod payload;
pub use payload::PendingFlashBlock;

pub mod sequence;
pub use sequence::{
    FlashBlockCompleteSequence, FlashBlockPendingSequence, SequenceExecutionOutcome,
};

pub mod service;
pub use service::{FlashBlockBuildInfo, FlashBlockService};

mod cache;
mod metrics;
mod worker;

pub(crate) use metrics::{FlashBlockServiceMetrics, record_build_skip, record_insert};

pub mod ws;
pub use ws::{FlashBlockDecoder, WsConnect, WsFlashBlockStream};

pub type PendingBlockRx<N> = tokio::sync::watch::Receiver<Option<PendingFlashBlock<N>>>;

pub type FlashBlockCompleteSequenceRx<P> =
    tokio::sync::broadcast::Receiver<FlashBlockCompleteSequence<P>>;

pub type InProgressFlashBlockRx = tokio::sync::watch::Receiver<Option<FlashBlockBuildInfo>>;

use reth_primitives_traits::NodePrimitives;
use std::sync::Arc;

#[derive(Debug)]
pub struct FlashblocksListeners<N: NodePrimitives, P: FlashblockPayload> {
    pub pending_block_rx: PendingBlockRx<N>,
    pub flashblocks_sequence: tokio::sync::broadcast::Sender<FlashBlockCompleteSequence<P>>,
    pub in_progress_rx: InProgressFlashBlockRx,
    pub received_flashblocks: tokio::sync::broadcast::Sender<Arc<P>>,
}

impl<N: NodePrimitives, P: FlashblockPayload> FlashblocksListeners<N, P> {
    pub const fn new(
        pending_block_rx: PendingBlockRx<N>,
        flashblocks_sequence: tokio::sync::broadcast::Sender<FlashBlockCompleteSequence<P>>,
        in_progress_rx: InProgressFlashBlockRx,
        received_flashblocks: tokio::sync::broadcast::Sender<Arc<P>>,
    ) -> Self {
        Self { pending_block_rx, flashblocks_sequence, in_progress_rx, received_flashblocks }
    }
}

use crate::{
    primitives::header::BlsPublicKey, sequencer::signing::BlsSignature,
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::{crypto::RecoveryError, transaction::Recovered};
use alloy_eips::{
    eip2718::WithEncoded,
    eip4895::{Withdrawal, Withdrawals},
};
use alloy_primitives::{Address, B256, Bloom, Bytes, U256};
use reth::rpc::types::engine::PayloadId;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainFlashblockPayloadBase {
    pub parent_beacon_block_root: B256,
    pub parent_hash: B256,
    pub fee_recipient: Address,
    pub prev_randao: B256,
    #[serde(with = "alloy_serde::quantity")]
    pub block_number: u64,
    #[serde(with = "alloy_serde::quantity")]
    pub gas_limit: u64,
    #[serde(with = "alloy_serde::quantity")]
    pub timestamp: u64,
    pub extra_data: Bytes,
    pub base_fee_per_gas: U256,
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
}

impl FlashblockPayloadBase for BerachainFlashblockPayloadBase {
    fn parent_hash(&self) -> B256 {
        self.parent_hash
    }

    fn block_number(&self) -> u64 {
        self.block_number
    }

    fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainFlashblockPayloadDiff {
    pub transactions: Vec<Bytes>,
    pub withdrawals: Vec<Withdrawal>,
}

impl FlashblockDiff for BerachainFlashblockPayloadDiff {
    fn block_hash(&self) -> B256 {
        B256::ZERO
    }

    fn state_root(&self) -> B256 {
        B256::ZERO
    }

    fn gas_used(&self) -> u64 {
        0
    }

    fn logs_bloom(&self) -> &Bloom {
        &Bloom::ZERO
    }

    fn receipts_root(&self) -> B256 {
        B256::ZERO
    }

    fn transactions_raw(&self) -> &[Bytes] {
        &self.transactions
    }

    fn withdrawals(&self) -> Option<&Withdrawals> {
        None
    }

    fn withdrawals_root(&self) -> Option<B256> {
        None
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainFlashblockPayloadMetadata {
    pub block_number: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainFlashblockPayload {
    pub payload_id: PayloadId,
    pub index: u64,
    pub base: Option<BerachainFlashblockPayloadBase>,
    pub diff: BerachainFlashblockPayloadDiff,
    pub metadata: BerachainFlashblockPayloadMetadata,
    #[serde(with = "signature_serde")]
    pub signature: BlsSignature,
    pub is_last: bool,
}

mod signature_serde {
    use super::BlsSignature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(sig: &BlsSignature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{}", hex::encode(sig)))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BlsSignature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_str = String::deserialize(deserializer)?;
        let hex_str = hex_str.trim_start_matches("0x");
        let bytes = hex::decode(hex_str).map_err(serde::de::Error::custom)?;
        if bytes.len() != 96 {
            return Err(serde::de::Error::custom(format!("expected 96 bytes, got {}", bytes.len())));
        }
        let mut arr = [0u8; 96];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

impl BerachainFlashblockPayload {
    pub const fn block_number(&self) -> u64 {
        self.metadata.block_number
    }
}

impl FlashblockPayload for BerachainFlashblockPayload {
    type Base = BerachainFlashblockPayloadBase;
    type Diff = BerachainFlashblockPayloadDiff;
    type SignedTx = BerachainTxEnvelope;

    fn index(&self) -> u64 {
        self.index
    }

    fn payload_id(&self) -> PayloadId {
        self.payload_id
    }

    fn base(&self) -> Option<&Self::Base> {
        self.base.as_ref()
    }

    fn diff(&self) -> &Self::Diff {
        &self.diff
    }

    fn block_number(&self) -> u64 {
        Self::block_number(self)
    }

    fn recover_transactions(
        &self,
    ) -> impl Iterator<Item = Result<WithEncoded<Recovered<Self::SignedTx>>, RecoveryError>> {
        use alloy_consensus::transaction::SignerRecoverable;
        use alloy_eips::Decodable2718;

        self.diff.transactions.clone().into_iter().map(|raw| {
            let tx = BerachainTxEnvelope::decode_2718(&mut raw.as_ref())
                .map_err(RecoveryError::from_source)?;
            tx.try_into_recovered().map(|tx| tx.into_encoded_with(raw.clone()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flashblocks::test_utils::BerachainTestFlashBlockFactory;

    #[test]
    fn test_flashblock_payload_serde_roundtrip() {
        let factory = BerachainTestFlashBlockFactory::new();
        let fb =
            factory.flashblock_at(0).transactions(vec![Bytes::from_static(&[1, 2, 3])]).build();

        let serialized = serde_json::to_string(&fb).expect("serialize");
        let deserialized: BerachainFlashblockPayload =
            serde_json::from_str(&serialized).expect("deserialize");

        assert_eq!(fb, deserialized);
    }

    #[test]
    fn test_transaction_aggregation_across_flashblocks() {
        let factory = BerachainTestFlashBlockFactory::new();

        let fb0 = factory
            .flashblock_at(0)
            .transactions(vec![Bytes::from_static(&[1, 2, 3]), Bytes::from_static(&[4, 5, 6])])
            .build();
        let fb1 = factory
            .flashblock_after(&fb0)
            .transactions(vec![Bytes::from_static(&[7, 8, 9])])
            .build();

        let complete = FlashBlockCompleteSequence::new(vec![fb0, fb1], None).unwrap();
        let all_txs = complete.all_transactions();

        assert_eq!(all_txs.len(), 3);
        assert_eq!(all_txs[0], Bytes::from_static(&[1, 2, 3]));
        assert_eq!(all_txs[1], Bytes::from_static(&[4, 5, 6]));
        assert_eq!(all_txs[2], Bytes::from_static(&[7, 8, 9]));
    }

    #[test]
    fn test_berachain_payload_base_preserved() {
        let factory = BerachainTestFlashBlockFactory::new();
        let pubkey = BlsPublicKey::random();

        let fb0 = factory.flashblock_at(0).prev_proposer_pubkey(Some(pubkey)).build();

        let complete = FlashBlockCompleteSequence::new(vec![fb0], None).unwrap();
        assert_eq!(complete.payload_base().prev_proposer_pubkey, Some(pubkey));
    }
}
