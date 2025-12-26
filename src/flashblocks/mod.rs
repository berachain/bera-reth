#[cfg(test)]
pub mod test_utils;

use crate::{primitives::header::BlsPublicKey, transaction::BerachainTxEnvelope};
use alloy_consensus::{crypto::RecoveryError, transaction::Recovered};
use alloy_eips::{
    eip2718::WithEncoded,
    eip4895::{Withdrawal, Withdrawals},
};
use alloy_primitives::{Address, B256, Bloom, Bytes, U256};
use reth::rpc::types::engine::PayloadId;
use reth_optimism_flashblocks::{FlashblockDiff, FlashblockPayload, FlashblockPayloadBase};

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainFlashblockPayloadBase {
    /// Parent beacon block root.
    pub parent_beacon_block_root: B256,
    /// Hash of the parent block.
    pub parent_hash: B256,
    /// Address that receives fees for this block.
    pub fee_recipient: Address,
    /// The previous randao value.
    pub prev_randao: B256,
    /// Block number.
    #[serde(with = "alloy_serde::quantity")]
    pub block_number: u64,
    /// Gas limit for this block.
    #[serde(with = "alloy_serde::quantity")]
    pub gas_limit: u64,
    /// Block timestamp.
    #[serde(with = "alloy_serde::quantity")]
    pub timestamp: u64,
    /// Extra data for the block.
    pub extra_data: Bytes,
    /// Base fee per gas for this block.
    pub base_fee_per_gas: U256,
    /// Berachain specific BlsPublicKey
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
    /// The state root of the block.
    pub state_root: B256,
    /// The receipts root of the block.
    pub receipts_root: B256,
    /// The logs bloom of the block.
    pub logs_bloom: Bloom,
    /// The gas used of the block.
    #[serde(with = "alloy_serde::quantity")]
    pub gas_used: u64,
    /// The block hash of the block.
    pub block_hash: B256,
    /// The transactions of the block.
    pub transactions: Vec<Bytes>,
    /// Array of [`Withdrawal`] enabled with V2
    pub withdrawals: Vec<Withdrawal>,
    /// The withdrawals root of the block.
    pub withdrawals_root: B256,
    /// The estimated cumulative blob gas used for the block. Introduced in Jovian.
    /// spec: <https://docs.optimism.io/notices/upgrade-17#block-header-changes>
    /// Defaults to 0 if not present (for pre-Jovian blocks).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "alloy_serde::quantity::opt")]
    pub blob_gas_used: Option<u64>,
}

impl FlashblockDiff for BerachainFlashblockPayloadDiff {
    fn block_hash(&self) -> B256 {
        self.block_hash
    }

    fn state_root(&self) -> B256 {
        self.state_root
    }

    fn gas_used(&self) -> u64 {
        self.gas_used
    }

    fn logs_bloom(&self) -> &Bloom {
        &self.logs_bloom
    }

    fn receipts_root(&self) -> B256 {
        self.receipts_root
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

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainFlashblockPayload {
    pub payload_id: PayloadId,
    pub index: u64,
    pub base: Option<BerachainFlashblockPayloadBase>,
    pub diff: BerachainFlashblockPayloadDiff,
    pub metadata: BerachainFlashblockPayloadMetadata,
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

    fn base(&self) -> Option<Self::Base> {
        self.base.clone()
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
    use reth_optimism_flashblocks::{FlashBlockCompleteSequence, FlashBlockPendingSequence};

    mod serde_tests {
        use super::*;

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
        fn test_flashblock_payload_base_serde_roundtrip() {
            let base = BerachainFlashblockPayloadBase {
                parent_beacon_block_root: B256::random(),
                parent_hash: B256::random(),
                fee_recipient: Address::random(),
                prev_randao: B256::random(),
                block_number: 100,
                gas_limit: 30_000_000,
                timestamp: 1_000_000,
                extra_data: Bytes::from_static(&[0x42]),
                base_fee_per_gas: U256::from(1_000_000_000u64),
                prev_proposer_pubkey: Some(BlsPublicKey::random()),
            };

            let serialized = serde_json::to_string(&base).expect("serialize");
            let deserialized: BerachainFlashblockPayloadBase =
                serde_json::from_str(&serialized).expect("deserialize");

            assert_eq!(base, deserialized);
        }

        #[test]
        fn test_flashblock_payload_diff_serde_roundtrip() {
            let diff = BerachainFlashblockPayloadDiff {
                state_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::default(),
                gas_used: 21000,
                block_hash: B256::random(),
                transactions: vec![Bytes::from_static(&[1, 2, 3])],
                withdrawals: vec![],
                withdrawals_root: B256::ZERO,
                blob_gas_used: Some(131072),
            };

            let serialized = serde_json::to_string(&diff).expect("serialize");
            let deserialized: BerachainFlashblockPayloadDiff =
                serde_json::from_str(&serialized).expect("deserialize");

            assert_eq!(diff, deserialized);
        }

        #[test]
        fn test_flashblock_sequence_serde_roundtrip() {
            let factory = BerachainTestFlashBlockFactory::new();
            let fb0 = factory.flashblock_at(0).build();
            let fb1 = factory.flashblock_after(&fb0).build();

            let fbs = vec![fb0, fb1];
            let serialized = serde_json::to_string(&fbs).expect("serialize");
            let deserialized: Vec<BerachainFlashblockPayload> =
                serde_json::from_str(&serialized).expect("deserialize");

            assert_eq!(fbs.len(), deserialized.len());
            assert_eq!(fbs[0], deserialized[0]);
            assert_eq!(fbs[1], deserialized[1]);
        }
    }

    mod pending_sequence_tests {
        use super::*;

        #[test]
        fn test_insert_index_zero_creates_new_sequence() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();
            let fb0 = factory.flashblock_at(0).build();
            let payload_id = fb0.payload_id;

            sequence.insert(fb0);

            assert_eq!(sequence.count(), 1);
            assert_eq!(sequence.block_number(), Some(100));
            assert_eq!(sequence.payload_id(), Some(payload_id));
        }

        #[test]
        fn test_insert_followup_same_block_and_payload() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            sequence.insert(fb0.clone());

            let fb1 = factory.flashblock_after(&fb0).build();
            sequence.insert(fb1.clone());

            let fb2 = factory.flashblock_after(&fb1).build();
            sequence.insert(fb2);

            assert_eq!(sequence.count(), 3);
            assert_eq!(sequence.index(), Some(2));
        }

        #[test]
        fn test_insert_ignores_different_block_number() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            sequence.insert(fb0.clone());

            let fb1 = factory.flashblock_after(&fb0).block_number(101).build();
            sequence.insert(fb1);

            assert_eq!(sequence.count(), 1);
            assert_eq!(sequence.block_number(), Some(100));
        }

        #[test]
        fn test_insert_ignores_different_payload_id() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            let payload_id1 = fb0.payload_id;
            sequence.insert(fb0.clone());

            let payload_id2 = PayloadId::new([2u8; 8]);
            let fb1 = factory.flashblock_after(&fb0).payload_id(payload_id2).build();
            sequence.insert(fb1);

            assert_eq!(sequence.count(), 1);
            assert_eq!(sequence.payload_id(), Some(payload_id1));
        }

        #[test]
        fn test_finalize_empty_sequence_fails() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let result = sequence.finalize();

            assert!(result.is_err());
        }

        #[test]
        fn test_finalize_clears_pending_state() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            sequence.insert(fb0);

            assert_eq!(sequence.count(), 1);

            let _complete = sequence.finalize().unwrap();

            assert_eq!(sequence.count(), 0);
            assert_eq!(sequence.block_number(), None);
        }
    }

    mod complete_sequence_tests {
        use super::*;

        #[test]
        fn test_new_empty_sequence_fails() {
            let result =
                FlashBlockCompleteSequence::<BerachainFlashblockPayload>::new(vec![], None);
            assert!(result.is_err());
        }

        #[test]
        fn test_new_requires_base_at_index_zero() {
            let factory = BerachainTestFlashBlockFactory::new();
            let mut fb0_no_base = factory.flashblock_at(1).build();
            fb0_no_base.index = 0;
            fb0_no_base.base = None;

            let result = FlashBlockCompleteSequence::new(vec![fb0_no_base], None);
            assert!(result.is_err());
        }

        #[test]
        fn test_new_validates_successive_indices() {
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            let fb2 = factory.flashblock_after(&fb0).index(2).build();

            let result = FlashBlockCompleteSequence::new(vec![fb0, fb2], None);
            assert!(result.is_err());
        }

        #[test]
        fn test_new_valid_single_flashblock() {
            let factory = BerachainTestFlashBlockFactory::new();
            let fb0 = factory.flashblock_at(0).build();

            let result = FlashBlockCompleteSequence::new(vec![fb0], None);
            assert!(result.is_ok());

            let complete = result.unwrap();
            assert_eq!(complete.count(), 1);
            assert_eq!(complete.block_number(), 100);
        }

        #[test]
        fn test_new_valid_multiple_flashblocks() {
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            let fb1 = factory.flashblock_after(&fb0).build();
            let fb2 = factory.flashblock_after(&fb1).build();

            let result = FlashBlockCompleteSequence::new(vec![fb0, fb1, fb2], None);
            assert!(result.is_ok());

            let complete = result.unwrap();
            assert_eq!(complete.count(), 3);
            assert_eq!(complete.last().index(), 2);
        }

        #[test]
        fn test_all_transactions_aggregates_correctly() {
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
        fn test_berachain_specific_payload_base_preserved() {
            let factory = BerachainTestFlashBlockFactory::new();
            let pubkey = BlsPublicKey::random();

            let fb0 = factory.flashblock_at(0).prev_proposer_pubkey(Some(pubkey)).build();

            let complete = FlashBlockCompleteSequence::new(vec![fb0], None).unwrap();
            let base = complete.payload_base();

            assert_eq!(base.prev_proposer_pubkey, Some(pubkey));
        }
    }

    mod sequence_conversion_tests {
        use super::*;

        #[test]
        fn test_try_from_pending_to_complete_valid() {
            let mut pending: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            pending.insert(fb0);

            let complete: Result<FlashBlockCompleteSequence<BerachainFlashblockPayload>, _> =
                pending.try_into();
            assert!(complete.is_ok());
            assert_eq!(complete.unwrap().count(), 1);
        }

        #[test]
        fn test_try_from_pending_to_complete_empty_fails() {
            let pending: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();

            let complete: Result<FlashBlockCompleteSequence<BerachainFlashblockPayload>, _> =
                pending.try_into();
            assert!(complete.is_err());
        }

        #[test]
        fn test_finalize_multiple_times_after_refill() {
            let mut sequence: FlashBlockPendingSequence<BerachainFlashblockPayload> =
                FlashBlockPendingSequence::new();
            let factory = BerachainTestFlashBlockFactory::new();

            let fb0 = factory.flashblock_at(0).build();
            sequence.insert(fb0);

            let complete1 = sequence.finalize().unwrap();
            assert_eq!(complete1.count(), 1);

            let fb1 = factory.flashblock_for_next_block(&complete1.last().clone()).build();
            sequence.insert(fb1);

            let complete2 = sequence.finalize().unwrap();
            assert_eq!(complete2.count(), 1);
            assert_eq!(complete2.block_number(), 101);
        }
    }
}
