use alloy_consensus::Header;
use alloy_primitives::{Address, B64, B256, BlockNumber, Bloom, Bytes, Sealable, U256};
use alloy_rlp::{Decodable, Encodable};
use bytes::BufMut;
use reth_codecs::Compact;
use reth_primitives_traits::{BlockHeader, InMemorySize, serde_bincode_compat::SerdeBincodeCompat};
use serde::{Deserialize, Serialize};

/// Berachain block header with additional fields for consensus
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct BerachainHeader {
    /// The Keccak 256-bit hash of the parent block's header, in its entirety.
    pub parent_hash: B256,
    /// The Keccak 256-bit hash of the ommers list portion of this block.
    pub ommers_hash: B256,
    /// The 160-bit address to which all fees collected from the successful mining of this block be
    /// transferred.
    pub beneficiary: Address,
    /// The Keccak 256-bit hash of the root node of the state trie, after all transactions are
    /// executed and finalizations are applied.
    pub state_root: B256,
    /// The Keccak 256-bit hash of the root node of the trie structure populated with each
    /// transaction in the transactions list portion of the block.
    pub transactions_root: B256,
    /// The Keccak 256-bit hash of the root node of the trie structure populated with the receipts
    /// of each transaction in the transactions list portion of the block.
    pub receipts_root: B256,
    /// The Keccak 256-bit hash of the withdrawals list portion of this block.
    pub withdrawals_root: Option<B256>,
    /// The Bloom filter composed from indexable information (logger address and log topics)
    /// contained in each log entry from the receipt of each transaction in the transactions list.
    pub logs_bloom: Bloom,
    /// A scalar value corresponding to the difficulty level of this block.
    pub difficulty: U256,
    /// A scalar value equal to the number of ancestor blocks. The genesis block has a number of
    /// zero.
    pub number: u64,
    /// A scalar value equal to the current limit of gas expenditure per block.
    pub gas_limit: u64,
    /// A scalar value equal to the total amount of gas used in transactions in this block.
    pub gas_used: u64,
    /// A scalar value equal to the reasonable output of Unix's time() at this block's inception.
    pub timestamp: u64,
    /// A 256-bit hash which, combined with the nonce, proves that a sufficient amount of
    /// computation has been carried out on this block.
    pub mix_hash: B256,
    /// A 64-bit value which, combined with the mixhash, proves that a sufficient amount of
    /// computation has been carried out on this block.
    pub nonce: B64,
    /// A scalar representing EIP1559 base fee which can move up or down each block according to a
    /// formula which is a function of gas used in parent block and gas target.
    pub base_fee_per_gas: Option<u64>,
    /// The total amount of blob gas consumed by the transactions within the block, added in
    /// EIP-4844.
    pub blob_gas_used: Option<u64>,
    /// A running total of blob gas consumed in excess of the target, prior to the block.
    pub excess_blob_gas: Option<u64>,
    /// The hash of the parent beacon block's root is included in execution blocks, as proposed by
    /// EIP-4788.
    pub parent_beacon_block_root: Option<B256>,
    /// The hash of the requests trie root, added in EIP-7685.
    pub requests_hash: Option<B256>,
    /// An arbitrary byte array containing data relevant to this block. This must be 32 bytes or
    /// fewer.
    pub extra_data: Bytes,
    /// Previous proposer public key for Berachain consensus.
    pub prev_proposer_pubkey: Option<B256>,
}

impl Encodable for BerachainHeader {
    fn encode(&self, out: &mut dyn BufMut) {
        todo!()
    }
}

impl Decodable for BerachainHeader {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        todo!()
    }
}

impl alloy_consensus::BlockHeader for BerachainHeader {
    fn parent_hash(&self) -> B256 {
        todo!()
    }

    fn ommers_hash(&self) -> B256 {
        todo!()
    }

    fn beneficiary(&self) -> Address {
        todo!()
    }

    fn state_root(&self) -> B256 {
        todo!()
    }

    fn transactions_root(&self) -> B256 {
        todo!()
    }

    fn receipts_root(&self) -> B256 {
        todo!()
    }

    fn withdrawals_root(&self) -> Option<B256> {
        todo!()
    }

    fn logs_bloom(&self) -> Bloom {
        todo!()
    }

    fn difficulty(&self) -> U256 {
        todo!()
    }

    fn number(&self) -> BlockNumber {
        todo!()
    }

    fn gas_limit(&self) -> u64 {
        todo!()
    }

    fn gas_used(&self) -> u64 {
        todo!()
    }

    fn timestamp(&self) -> u64 {
        todo!()
    }

    fn mix_hash(&self) -> Option<B256> {
        todo!()
    }

    fn nonce(&self) -> Option<B64> {
        todo!()
    }

    fn base_fee_per_gas(&self) -> Option<u64> {
        todo!()
    }

    fn blob_gas_used(&self) -> Option<u64> {
        todo!()
    }

    fn excess_blob_gas(&self) -> Option<u64> {
        todo!()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        todo!()
    }

    fn requests_hash(&self) -> Option<B256> {
        todo!()
    }

    fn extra_data(&self) -> &Bytes {
        todo!()
    }
}

impl Sealable for BerachainHeader {
    fn hash_slow(&self) -> B256 {
        todo!()
    }
}

impl InMemorySize for BerachainHeader {
    fn size(&self) -> usize {
        todo!()
    }
}

impl SerdeBincodeCompat for BerachainHeader {
    type BincodeRepr<'a> = ();

    fn as_repr(&self) -> Self::BincodeRepr<'_> {
        todo!()
    }

    fn from_repr(repr: Self::BincodeRepr<'_>) -> Self {
        todo!()
    }
}

impl AsRef<Self> for BerachainHeader {
    fn as_ref(&self) -> &Self {
        todo!()
    }
}

impl BlockHeader for BerachainHeader {}

impl Compact for BerachainHeader {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        todo!()
    }

    fn from_compact(buf: &[u8], len: usize) -> (Self, &[u8]) {
        todo!()
    }
}

impl From<&Header> for BerachainHeader {
    fn from(value: &Header) -> Self {
        BerachainHeader {
            parent_hash: value.parent_hash,
            ommers_hash: value.ommers_hash,
            beneficiary: value.beneficiary,
            state_root: value.state_root,
            transactions_root: value.transactions_root,
            receipts_root: value.receipts_root,
            withdrawals_root: value.withdrawals_root,
            logs_bloom: value.logs_bloom,
            difficulty: value.difficulty,
            number: value.number,
            gas_limit: value.gas_limit,
            gas_used: value.gas_used,
            timestamp: value.timestamp,
            mix_hash: value.mix_hash,
            nonce: value.nonce,
            base_fee_per_gas: value.base_fee_per_gas,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value.parent_beacon_block_root,
            requests_hash: value.requests_hash,
            extra_data: value.clone().extra_data,
            prev_proposer_pubkey: None,
        }
    }
}
