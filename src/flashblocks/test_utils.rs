//! Test utilities for creating flashblock payloads.

use crate::{
    flashblocks::{
        BerachainFlashblockPayload, BerachainFlashblockPayloadBase, BerachainFlashblockPayloadDiff,
        BerachainFlashblockPayloadMetadata,
    },
    primitives::header::BlsPublicKey,
};
use alloy_primitives::{Address, B256, Bloom, Bytes, U256};
use reth::rpc::types::engine::PayloadId;

#[derive(Debug)]
pub struct BerachainTestFlashBlockFactory {
    block_time: u64,
    base_timestamp: u64,
    current_block_number: u64,
}

impl BerachainTestFlashBlockFactory {
    pub fn new() -> Self {
        Self { block_time: 2, base_timestamp: 1_000_000, current_block_number: 100 }
    }

    pub fn with_block_number(mut self, block_number: u64) -> Self {
        self.current_block_number = block_number;
        self
    }

    pub fn flashblock_at(&self, index: u64) -> BerachainTestFlashBlockBuilder {
        self.builder().index(index).block_number(self.current_block_number)
    }

    pub fn flashblock_after(
        &self,
        previous: &BerachainFlashblockPayload,
    ) -> BerachainTestFlashBlockBuilder {
        let parent_hash =
            previous.base.as_ref().map(|b| b.parent_hash).unwrap_or(previous.diff.block_hash);

        self.builder()
            .index(previous.index + 1)
            .block_number(previous.metadata.block_number)
            .payload_id(previous.payload_id)
            .parent_hash(parent_hash)
            .timestamp(previous.base.as_ref().map(|b| b.timestamp).unwrap_or(self.base_timestamp))
    }

    pub fn flashblock_for_next_block(
        &self,
        previous: &BerachainFlashblockPayload,
    ) -> BerachainTestFlashBlockBuilder {
        let prev_timestamp =
            previous.base.as_ref().map(|b| b.timestamp).unwrap_or(self.base_timestamp);
        let prev_block_number = previous.metadata.block_number;

        self.builder()
            .index(0)
            .block_number(prev_block_number + 1)
            .payload_id(PayloadId::new(B256::random().0[0..8].try_into().unwrap()))
            .parent_hash(previous.diff.block_hash)
            .timestamp(prev_timestamp + self.block_time)
    }

    fn builder(&self) -> BerachainTestFlashBlockBuilder {
        BerachainTestFlashBlockBuilder {
            index: 0,
            block_number: self.current_block_number,
            payload_id: PayloadId::new([1u8; 8]),
            parent_hash: B256::random(),
            timestamp: self.base_timestamp,
            gas_limit: 30_000_000,
            transactions: vec![],
            prev_proposer_pubkey: Some(BlsPublicKey::random()),
        }
    }
}

impl Default for BerachainTestFlashBlockFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct BerachainTestFlashBlockBuilder {
    index: u64,
    block_number: u64,
    payload_id: PayloadId,
    parent_hash: B256,
    timestamp: u64,
    gas_limit: u64,
    transactions: Vec<Bytes>,
    prev_proposer_pubkey: Option<BlsPublicKey>,
}

impl BerachainTestFlashBlockBuilder {
    pub fn index(mut self, index: u64) -> Self {
        self.index = index;
        self
    }

    pub fn block_number(mut self, block_number: u64) -> Self {
        self.block_number = block_number;
        self
    }

    pub fn payload_id(mut self, payload_id: PayloadId) -> Self {
        self.payload_id = payload_id;
        self
    }

    pub fn parent_hash(mut self, parent_hash: B256) -> Self {
        self.parent_hash = parent_hash;
        self
    }

    pub fn timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = timestamp;
        self
    }

    pub fn transactions(mut self, transactions: Vec<Bytes>) -> Self {
        self.transactions = transactions;
        self
    }

    pub fn prev_proposer_pubkey(mut self, pubkey: Option<BlsPublicKey>) -> Self {
        self.prev_proposer_pubkey = pubkey;
        self
    }

    pub fn build(self) -> BerachainFlashblockPayload {
        let base = if self.index == 0 {
            Some(BerachainFlashblockPayloadBase {
                parent_hash: self.parent_hash,
                parent_beacon_block_root: B256::random(),
                fee_recipient: Address::default(),
                prev_randao: B256::random(),
                block_number: self.block_number,
                gas_limit: self.gas_limit,
                timestamp: self.timestamp,
                extra_data: Default::default(),
                base_fee_per_gas: U256::from(1_000_000_000u64),
                prev_proposer_pubkey: self.prev_proposer_pubkey,
            })
        } else {
            None
        };

        BerachainFlashblockPayload {
            index: self.index,
            payload_id: self.payload_id,
            base,
            diff: BerachainFlashblockPayloadDiff {
                block_hash: B256::random(),
                state_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::default(),
                gas_used: 0,
                transactions: self.transactions,
                withdrawals: vec![],
                withdrawals_root: B256::ZERO,
                blob_gas_used: None,
            },
            metadata: BerachainFlashblockPayloadMetadata { block_number: self.block_number },
        }
    }
}
