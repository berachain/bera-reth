use alloy_consensus::Header;
use alloy_primitives::{
    Address, B64, B256, BlockNumber, Bloom, Bytes, FixedBytes, Sealable, U256, keccak256,
};
use alloy_rlp::{Decodable, Encodable, length_of_length};
use bytes::BufMut;
use reth_codecs::Compact;
use reth_db_api::table::{Compress, Decompress};
use reth_primitives_traits::{BlockHeader, InMemorySize, serde_bincode_compat::RlpBincode};
use serde::{Deserialize, Serialize};

/// 48-byte BLS12-381 public key for Berachain consensus
pub type BlsPublicKey = FixedBytes<48>;

/// Berachain block header with additional fields for consensus
/// TODO: All of the implementations here need to be properly tested.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Compact)]
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
    pub nonce: u64,
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
    /// Previous proposer public key for Berachain consensus.
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
    /// An arbitrary byte array containing data relevant to this block. This must be 32 bytes or
    /// fewer. Must be last for Compact derive.
    pub extra_data: Bytes,
}

impl BerachainHeader {
    fn header_payload_length(&self) -> usize {
        let mut length = 0;
        length += self.parent_hash.length();
        length += self.ommers_hash.length();
        length += self.beneficiary.length();
        length += self.state_root.length();
        length += self.transactions_root.length();
        length += self.receipts_root.length();
        length += self.logs_bloom.length();
        length += self.difficulty.length();
        length += U256::from(self.number).length();
        length += U256::from(self.gas_limit).length();
        length += U256::from(self.gas_used).length();
        length += self.timestamp.length();
        length += self.extra_data.length();
        length += self.mix_hash.length();
        length += self.nonce.length();

        if let Some(base_fee) = self.base_fee_per_gas {
            length += U256::from(base_fee).length();
        }

        if let Some(root) = self.withdrawals_root {
            length += root.length();
        }

        if let Some(blob_gas_used) = self.blob_gas_used {
            length += U256::from(blob_gas_used).length();
        }

        if let Some(excess_blob_gas) = self.excess_blob_gas {
            length += U256::from(excess_blob_gas).length();
        }

        if let Some(parent_beacon_block_root) = self.parent_beacon_block_root {
            length += parent_beacon_block_root.length();
        }

        if let Some(requests_hash) = self.requests_hash {
            length += requests_hash.length();
        }

        if let Some(prev_proposer_pubkey) = self.prev_proposer_pubkey {
            length += prev_proposer_pubkey.length();
        }

        length
    }
}

impl Encodable for BerachainHeader {
    fn encode(&self, out: &mut dyn BufMut) {
        let list_header =
            alloy_rlp::Header { list: true, payload_length: self.header_payload_length() };
        list_header.encode(out);
        self.parent_hash.encode(out);
        self.ommers_hash.encode(out);
        self.beneficiary.encode(out);
        self.state_root.encode(out);
        self.transactions_root.encode(out);
        self.receipts_root.encode(out);
        self.logs_bloom.encode(out);
        self.difficulty.encode(out);
        U256::from(self.number).encode(out);
        U256::from(self.gas_limit).encode(out);
        U256::from(self.gas_used).encode(out);
        self.timestamp.encode(out);
        self.extra_data.encode(out);
        self.mix_hash.encode(out);
        self.nonce.encode(out);

        if let Some(base_fee) = self.base_fee_per_gas {
            U256::from(base_fee).encode(out);
        }

        if let Some(root) = self.withdrawals_root {
            root.encode(out);
        }

        if let Some(blob_gas_used) = self.blob_gas_used {
            U256::from(blob_gas_used).encode(out);
        }

        if let Some(excess_blob_gas) = self.excess_blob_gas {
            U256::from(excess_blob_gas).encode(out);
        }

        if let Some(parent_beacon_block_root) = self.parent_beacon_block_root {
            parent_beacon_block_root.encode(out);
        }

        if let Some(requests_hash) = self.requests_hash {
            requests_hash.encode(out);
        }

        if let Some(prev_proposer_pubkey) = self.prev_proposer_pubkey {
            prev_proposer_pubkey.encode(out);
        }
    }

    fn length(&self) -> usize {
        let mut length = 0;
        length += self.header_payload_length();
        length += length_of_length(length);
        length
    }
}

impl Decodable for BerachainHeader {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let rlp_head = alloy_rlp::Header::decode(buf)?;
        if !rlp_head.list {
            return Err(alloy_rlp::Error::UnexpectedString);
        }
        let started_len = buf.len();
        let mut this = Self {
            parent_hash: Decodable::decode(buf)?,
            ommers_hash: Decodable::decode(buf)?,
            beneficiary: Decodable::decode(buf)?,
            state_root: Decodable::decode(buf)?,
            transactions_root: Decodable::decode(buf)?,
            receipts_root: Decodable::decode(buf)?,
            logs_bloom: Decodable::decode(buf)?,
            difficulty: Decodable::decode(buf)?,
            number: u64::decode(buf)?,
            gas_limit: u64::decode(buf)?,
            gas_used: u64::decode(buf)?,
            timestamp: Decodable::decode(buf)?,
            extra_data: Decodable::decode(buf)?,
            mix_hash: Decodable::decode(buf)?,
            nonce: u64::decode(buf)?,
            base_fee_per_gas: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            requests_hash: None,
            prev_proposer_pubkey: None,
        };

        if started_len - buf.len() < rlp_head.payload_length {
            this.base_fee_per_gas = Some(u64::decode(buf)?);
        }

        if started_len - buf.len() < rlp_head.payload_length {
            this.withdrawals_root = Some(Decodable::decode(buf)?);
        }

        if started_len - buf.len() < rlp_head.payload_length {
            this.blob_gas_used = Some(u64::decode(buf)?);
        }

        if started_len - buf.len() < rlp_head.payload_length {
            this.excess_blob_gas = Some(u64::decode(buf)?);
        }

        if started_len - buf.len() < rlp_head.payload_length {
            this.parent_beacon_block_root = Some(B256::decode(buf)?);
        }

        if started_len - buf.len() < rlp_head.payload_length {
            this.requests_hash = Some(B256::decode(buf)?);
        }

        if started_len - buf.len() < rlp_head.payload_length {
            this.prev_proposer_pubkey = Some(BlsPublicKey::decode(buf)?);
        }

        let consumed = started_len - buf.len();
        if consumed != rlp_head.payload_length {
            return Err(alloy_rlp::Error::ListLengthMismatch {
                expected: rlp_head.payload_length,
                got: consumed,
            });
        }
        Ok(this)
    }
}

impl alloy_consensus::BlockHeader for BerachainHeader {
    fn parent_hash(&self) -> B256 {
        self.parent_hash
    }

    fn ommers_hash(&self) -> B256 {
        self.ommers_hash
    }

    fn beneficiary(&self) -> Address {
        self.beneficiary
    }

    fn state_root(&self) -> B256 {
        self.state_root
    }

    fn transactions_root(&self) -> B256 {
        self.transactions_root
    }

    fn receipts_root(&self) -> B256 {
        self.receipts_root
    }

    fn withdrawals_root(&self) -> Option<B256> {
        self.withdrawals_root
    }

    fn logs_bloom(&self) -> Bloom {
        self.logs_bloom
    }

    fn difficulty(&self) -> U256 {
        self.difficulty
    }

    fn number(&self) -> BlockNumber {
        self.number
    }

    fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    fn gas_used(&self) -> u64 {
        self.gas_used
    }

    fn timestamp(&self) -> u64 {
        self.timestamp
    }

    fn mix_hash(&self) -> Option<B256> {
        Some(self.mix_hash)
    }

    fn nonce(&self) -> Option<B64> {
        Some(self.nonce.into())
    }

    fn base_fee_per_gas(&self) -> Option<u64> {
        self.base_fee_per_gas
    }

    fn blob_gas_used(&self) -> Option<u64> {
        self.blob_gas_used
    }

    fn excess_blob_gas(&self) -> Option<u64> {
        self.excess_blob_gas
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.parent_beacon_block_root
    }

    fn requests_hash(&self) -> Option<B256> {
        self.requests_hash
    }

    fn extra_data(&self) -> &Bytes {
        &self.extra_data
    }
}

impl Sealable for BerachainHeader {
    fn hash_slow(&self) -> B256 {
        let mut out = Vec::<u8>::new();
        self.encode(&mut out);
        keccak256(&out)
    }
}

impl InMemorySize for BerachainHeader {
    fn size(&self) -> usize {
        use core::mem;

        mem::size_of::<B256>() + // parent_hash
        mem::size_of::<B256>() + // ommers_hash
        mem::size_of::<Address>() + // beneficiary
        mem::size_of::<B256>() + // state_root
        mem::size_of::<B256>() + // transactions_root
        mem::size_of::<B256>() + // receipts_root
        mem::size_of::<Option<B256>>() + // withdrawals_root
        mem::size_of::<Bloom>() + // logs_bloom
        mem::size_of::<U256>() + // difficulty
        mem::size_of::<BlockNumber>() + // number
        mem::size_of::<u64>() + // gas_limit
        mem::size_of::<u64>() + // gas_used
        mem::size_of::<u64>() + // timestamp
        mem::size_of::<B256>() + // mix_hash
        mem::size_of::<u64>() + // nonce
        mem::size_of::<Option<u64>>() + // base_fee_per_gas
        mem::size_of::<Option<u64>>() + // blob_gas_used
        mem::size_of::<Option<u64>>() + // excess_blob_gas
        mem::size_of::<Option<B256>>() + // parent_beacon_block_root
        mem::size_of::<Option<B256>>() + // requests_hash
        mem::size_of::<Option<BlsPublicKey>>() + // prev_proposer_pubkey
        self.extra_data.len() // extra_data
    }
}

impl RlpBincode for BerachainHeader {}

impl AsRef<Self> for BerachainHeader {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl BlockHeader for BerachainHeader {}

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
            nonce: value.nonce.into(),
            base_fee_per_gas: value.base_fee_per_gas,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value.parent_beacon_block_root,
            requests_hash: value.requests_hash,
            prev_proposer_pubkey: None,
            extra_data: value.clone().extra_data,
        }
    }
}

impl From<Header> for BerachainHeader {
    fn from(value: Header) -> Self {
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
            nonce: value.nonce.into(),
            base_fee_per_gas: value.base_fee_per_gas,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value.parent_beacon_block_root,
            requests_hash: value.requests_hash,
            prev_proposer_pubkey: None,
            extra_data: value.extra_data,
        }
    }
}

// Database traits implementation
impl Compress for BerachainHeader {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        let _ = Compact::to_compact(self, buf);
    }
}

impl Decompress for BerachainHeader {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        let (obj, _) = Compact::from_compact(value, value.len());
        Ok(obj)
    }
}
