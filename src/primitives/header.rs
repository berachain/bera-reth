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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainHeader {
    /// The Keccak 256-bit hash of the parent block's header, in its entirety.
    pub parent_hash: B256,
    /// The Keccak 256-bit hash of the ommers list portion of this block.
    #[serde(rename = "sha3Uncles", alias = "ommersHash")]
    pub ommers_hash: B256,
    /// The 160-bit address to which all fees collected from the successful mining of this block be
    /// transferred.
    #[serde(rename = "miner", alias = "beneficiary")]
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
    #[serde(with = "alloy_serde::quantity")]
    pub number: u64,
    /// A scalar value equal to the current limit of gas expenditure per block.
    #[serde(with = "alloy_serde::quantity")]
    pub gas_limit: u64,
    /// A scalar value equal to the total amount of gas used in transactions in this block.
    #[serde(with = "alloy_serde::quantity")]
    pub gas_used: u64,
    /// A scalar value equal to the reasonable output of Unix's time() at this block's inception.
    #[serde(with = "alloy_serde::quantity")]
    pub timestamp: u64,
    /// A 256-bit hash which, combined with the nonce, proves that a sufficient amount of
    /// computation has been carried out on this block.
    pub mix_hash: B256,
    /// A 64-bit value which, combined with the mixhash, proves that a sufficient amount of
    /// computation has been carried out on this block.
    pub nonce: B64,
    /// A scalar representing EIP1559 base fee which can move up or down each block according to a
    /// formula which is a function of gas used in parent block and gas target.
    #[serde(default, with = "alloy_serde::quantity::opt", skip_serializing_if = "Option::is_none")]
    pub base_fee_per_gas: Option<u64>,
    /// The total amount of blob gas consumed by the transactions within the block, added in
    /// EIP-4844.
    #[serde(default, with = "alloy_serde::quantity::opt", skip_serializing_if = "Option::is_none")]
    pub blob_gas_used: Option<u64>,
    /// A running total of blob gas consumed in excess of the target, prior to the block.
    #[serde(default, with = "alloy_serde::quantity::opt", skip_serializing_if = "Option::is_none")]
    pub excess_blob_gas: Option<u64>,
    /// The hash of the parent beacon block's root is included in execution blocks, as proposed by
    /// EIP-4788.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_beacon_block_root: Option<B256>,
    /// The hash of the requests trie root, added in EIP-7685.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_hash: Option<B256>,
    /// Previous proposer public key for Berachain consensus.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "parentProposerPubkey",
        alias = "prevProposerPubkey"
    )]
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
            nonce: Decodable::decode(buf)?,
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
        Some(self.nonce)
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

/// This should only be called in genesis chainspec parsing, as it sets proposer_pub_key to None.
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
            nonce: value.nonce,
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

/// Internal header struct for Compact derive
///
/// This mirrors the pattern used in reth for alloy consensus Header at:
/// <https://github.com/paradigmxyz/reth/blob/main/crates/storage/codecs/src/alloy/header.rs>
///
/// The pattern is used because some field types (like B64) cannot derive Compact directly,
/// so we create an internal struct with compatible types (u64 for nonce) and bridge between them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Compact, Serialize, Deserialize)]
struct CompactBerachainHeader {
    parent_hash: B256,
    ommers_hash: B256,
    beneficiary: Address,
    state_root: B256,
    transactions_root: B256,
    receipts_root: B256,
    withdrawals_root: Option<B256>,
    logs_bloom: Bloom,
    difficulty: U256,
    number: BlockNumber,
    gas_limit: u64,
    gas_used: u64,
    timestamp: u64,
    mix_hash: B256,
    nonce: u64,
    base_fee_per_gas: Option<u64>,
    blob_gas_used: Option<u64>,
    excess_blob_gas: Option<u64>,
    parent_beacon_block_root: Option<B256>,
    extra_fields: Option<BerachainHeaderExt>,
    extra_data: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Compact, Serialize, Deserialize)]
pub(crate) struct BerachainHeaderExt {
    requests_hash: Option<B256>,
    prev_proposer_pubkey: Option<BlsPublicKey>,
}

impl BerachainHeaderExt {
    const fn into_option(self) -> Option<Self> {
        if self.requests_hash.is_some() || self.prev_proposer_pubkey.is_some() {
            Some(self)
        } else {
            None
        }
    }
}

impl Compact for BerachainHeader {
    /// Converts BerachainHeader to compact format using internal CompactBerachainHeader
    ///
    /// This follows the same pattern as reth's implementation for alloy consensus Header.
    /// See: <https://github.com/paradigmxyz/reth/blob/main/crates/storage/codecs/src/alloy/header.rs#L76-L107>
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        let extra_fields = BerachainHeaderExt {
            requests_hash: self.requests_hash,
            prev_proposer_pubkey: self.prev_proposer_pubkey,
        };

        let compact_header = CompactBerachainHeader {
            parent_hash: self.parent_hash,
            ommers_hash: self.ommers_hash,
            beneficiary: self.beneficiary,
            state_root: self.state_root,
            transactions_root: self.transactions_root,
            receipts_root: self.receipts_root,
            withdrawals_root: self.withdrawals_root,
            logs_bloom: self.logs_bloom,
            difficulty: self.difficulty,
            number: self.number,
            gas_limit: self.gas_limit,
            gas_used: self.gas_used,
            timestamp: self.timestamp,
            mix_hash: self.mix_hash,
            nonce: self.nonce.into(),
            base_fee_per_gas: self.base_fee_per_gas,
            blob_gas_used: self.blob_gas_used,
            excess_blob_gas: self.excess_blob_gas,
            parent_beacon_block_root: self.parent_beacon_block_root,
            extra_fields: extra_fields.into_option(),
            extra_data: self.extra_data.clone(),
        };
        compact_header.to_compact(buf)
    }

    /// Converts from compact format to BerachainHeader using internal CompactBerachainHeader
    ///
    /// This follows the same pattern as reth's implementation for alloy consensus Header.
    /// See: <https://github.com/paradigmxyz/reth/blob/main/crates/storage/codecs/src/alloy/header.rs#L109-L136>
    fn from_compact(buf: &[u8], len: usize) -> (Self, &[u8]) {
        let (header, _) = CompactBerachainHeader::from_compact(buf, len);

        let berachain_header = Self {
            parent_hash: header.parent_hash,
            ommers_hash: header.ommers_hash,
            beneficiary: header.beneficiary,
            state_root: header.state_root,
            transactions_root: header.transactions_root,
            receipts_root: header.receipts_root,
            withdrawals_root: header.withdrawals_root,
            logs_bloom: header.logs_bloom,
            difficulty: header.difficulty,
            number: header.number,
            gas_limit: header.gas_limit,
            gas_used: header.gas_used,
            timestamp: header.timestamp,
            mix_hash: header.mix_hash,
            nonce: header.nonce.into(),
            base_fee_per_gas: header.base_fee_per_gas,
            blob_gas_used: header.blob_gas_used,
            excess_blob_gas: header.excess_blob_gas,
            parent_beacon_block_root: header.parent_beacon_block_root,
            requests_hash: header.extra_fields.as_ref().and_then(|h| h.requests_hash),
            prev_proposer_pubkey: header.extra_fields.as_ref().and_then(|h| h.prev_proposer_pubkey),
            extra_data: header.extra_data,
        };

        (berachain_header, buf)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reth_header_compatability() {
        use alloy_primitives::hex::FromHex;

        // Test with various AlloyHeader configurations
        let test_cases = vec![
            // Basic header with minimal fields
            Header {
                parent_hash: B256::random(),
                ommers_hash: B256::from_hex(
                    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                )
                .unwrap(),
                beneficiary: Address::random(),
                state_root: B256::random(),
                transactions_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::random(),
                difficulty: U256::from(12345),
                number: 1000000,
                gas_limit: 30000000,
                gas_used: 21000,
                timestamp: 1640995200,
                extra_data: Bytes::from_hex("0x657468657265756d").unwrap(),
                mix_hash: B256::random(),
                nonce: B64::random(),
                base_fee_per_gas: None,
                withdrawals_root: None,
                blob_gas_used: None,
                excess_blob_gas: None,
                parent_beacon_block_root: None,
                requests_hash: None,
            },
            // Header with EIP-1559 (London fork)
            Header {
                parent_hash: B256::random(),
                ommers_hash: B256::from_hex(
                    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                )
                .unwrap(),
                beneficiary: Address::random(),
                state_root: B256::random(),
                transactions_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::random(),
                difficulty: U256::from(54321),
                number: 2000000,
                gas_limit: 30000000,
                gas_used: 15000000,
                timestamp: 1640995300,
                extra_data: Bytes::from_hex("0x626572612d72657468").unwrap(),
                mix_hash: B256::random(),
                nonce: B64::random(),
                base_fee_per_gas: Some(1000000000), // 1 gwei
                withdrawals_root: None,
                blob_gas_used: None,
                excess_blob_gas: None,
                parent_beacon_block_root: None,
                requests_hash: None,
            },
            // Header with Shanghai fork (withdrawals)
            Header {
                parent_hash: B256::from_hex(
                    "0x3234567890123456789012345678901234567890123456789012345678901234",
                )
                .unwrap(),
                ommers_hash: B256::from_hex(
                    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                )
                .unwrap(),
                beneficiary: Address::random(),
                state_root: B256::random(),
                transactions_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::random(),
                difficulty: U256::ZERO,
                number: 3000000,
                gas_limit: 30000000,
                gas_used: 20000000,
                timestamp: 1678886400,
                extra_data: Bytes::from_hex("0x62657261636861696e").unwrap(),
                mix_hash: B256::random(),
                nonce: B64::ZERO,
                base_fee_per_gas: Some(500000000), // 0.5 gwei
                withdrawals_root: Some(B256::random()),
                blob_gas_used: None,
                excess_blob_gas: None,
                parent_beacon_block_root: Some(B256::random()),
                requests_hash: None,
            },
            // Header with Cancun fork (blobs)
            Header {
                parent_hash: B256::random(),
                ommers_hash: B256::from_hex(
                    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                )
                .unwrap(),
                beneficiary: Address::random(),
                state_root: B256::random(),
                transactions_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::random(),
                difficulty: U256::ZERO,
                number: 4000000,
                gas_limit: 30000000,
                gas_used: 25000000,
                timestamp: 1710441600,
                extra_data: Bytes::from_hex("0x70726167756531").unwrap(),
                mix_hash: B256::random(),
                nonce: B64::ZERO,
                base_fee_per_gas: Some(2000000000), // 2 gwei
                withdrawals_root: Some(B256::random()),
                blob_gas_used: Some(131072),   // 128KB
                excess_blob_gas: Some(262144), // 256KB
                parent_beacon_block_root: Some(B256::random()),
                requests_hash: None,
            },
            // Header with Prague fork (requests)
            Header {
                parent_hash: B256::random(),
                ommers_hash: B256::from_hex(
                    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                )
                .unwrap(),
                beneficiary: Address::random(),
                state_root: B256::random(),
                transactions_root: B256::random(),
                receipts_root: B256::random(),
                logs_bloom: Bloom::random(),
                difficulty: U256::ZERO,
                number: 5000000,
                gas_limit: 30000000,
                gas_used: 28000000,
                timestamp: 1735689600, // Future timestamp
                extra_data: Bytes::from_hex("0x626572616368696b6974").unwrap(),
                mix_hash: B256::random(),
                nonce: B64::ZERO,
                base_fee_per_gas: Some(1500000000), // 1.5 gwei
                withdrawals_root: Some(B256::random()),
                blob_gas_used: Some(524288),    // 512KB
                excess_blob_gas: Some(1048576), // 1MB
                parent_beacon_block_root: Some(B256::random()),
                requests_hash: Some(B256::random()),
            },
        ];

        for (i, reth_header) in test_cases.iter().enumerate() {
            // Test compression and decompression
            let buf = &mut Vec::new();
            reth_header.compress_to_buf(buf);

            let decompressed = BerachainHeader::decompress(buf.as_slice()).unwrap();

            // Verify all standard Ethereum fields match exactly
            assert_eq!(
                decompressed.parent_hash, reth_header.parent_hash,
                "Test case {i}: parent_hash mismatch"
            );
            assert_eq!(
                decompressed.ommers_hash, reth_header.ommers_hash,
                "Test case {i}: ommers_hash mismatch"
            );
            assert_eq!(
                decompressed.beneficiary, reth_header.beneficiary,
                "Test case {i}: beneficiary mismatch"
            );
            assert_eq!(
                decompressed.state_root, reth_header.state_root,
                "Test case {i}: state_root mismatch"
            );
            assert_eq!(
                decompressed.transactions_root, reth_header.transactions_root,
                "Test case {i}: transactions_root mismatch"
            );
            assert_eq!(
                decompressed.receipts_root, reth_header.receipts_root,
                "Test case {i}: receipts_root mismatch"
            );
            assert_eq!(
                decompressed.withdrawals_root, reth_header.withdrawals_root,
                "Test case {i}: withdrawals_root mismatch"
            );
            assert_eq!(
                decompressed.logs_bloom, reth_header.logs_bloom,
                "Test case {i}: logs_bloom mismatch"
            );
            assert_eq!(
                decompressed.difficulty, reth_header.difficulty,
                "Test case {i}: difficulty mismatch"
            );
            assert_eq!(decompressed.number, reth_header.number, "Test case {i}: number mismatch");
            assert_eq!(
                decompressed.gas_limit, reth_header.gas_limit,
                "Test case {i}: gas_limit mismatch"
            );
            assert_eq!(
                decompressed.gas_used, reth_header.gas_used,
                "Test case {i}: gas_used mismatch"
            );
            assert_eq!(
                decompressed.timestamp, reth_header.timestamp,
                "Test case {i}: timestamp mismatch"
            );
            assert_eq!(
                decompressed.mix_hash, reth_header.mix_hash,
                "Test case {i}: mix_hash mismatch"
            );
            assert_eq!(decompressed.nonce, reth_header.nonce, "Test case {i}: nonce mismatch");
            assert_eq!(
                decompressed.base_fee_per_gas, reth_header.base_fee_per_gas,
                "Test case {i}: base_fee_per_gas mismatch"
            );
            assert_eq!(
                decompressed.blob_gas_used, reth_header.blob_gas_used,
                "Test case {i}: blob_gas_used mismatch"
            );
            assert_eq!(
                decompressed.excess_blob_gas, reth_header.excess_blob_gas,
                "Test case {i}: excess_blob_gas mismatch"
            );
            assert_eq!(
                decompressed.parent_beacon_block_root, reth_header.parent_beacon_block_root,
                "Test case {i}: parent_beacon_block_root mismatch"
            );
            assert_eq!(
                decompressed.requests_hash, reth_header.requests_hash,
                "Test case {i}: requests_hash mismatch"
            );
            assert_eq!(
                decompressed.extra_data, reth_header.extra_data,
                "Test case {i}: extra_data mismatch"
            );

            // Verify Berachain-specific field is None (backwards compatibility)
            assert_eq!(
                decompressed.prev_proposer_pubkey, None,
                "Test case {i}: prev_proposer_pubkey should be None for backwards compatibility"
            );
        }
    }

    #[test]
    fn test_berachain_header_extension_compatibility() {
        use alloy_primitives::hex::FromHex;

        // Test BerachainHeader with prev_proposer_pubkey field
        let berachain_header = BerachainHeader {
            parent_hash: B256::random(),
            ommers_hash: B256::from_hex(
                "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            )
            .unwrap(),
            beneficiary: Address::random(),
            state_root: B256::random(),
            transactions_root: B256::random(),
            receipts_root: B256::random(),
            withdrawals_root: Some(B256::random()),
            logs_bloom: Bloom::random(),
            difficulty: U256::ZERO,
            number: 1947953,
            gas_limit: 30000000,
            gas_used: 4458825,
            timestamp: 1721327232,
            mix_hash: B256::random(),
            nonce: B64::random(),
            base_fee_per_gas: Some(1000000000),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: Some(B256::random()),
            requests_hash: Some(B256::random()),
            prev_proposer_pubkey: Some(BlsPublicKey::random()),
            extra_data: Bytes::from_hex("0x626572616368696b6974").unwrap(),
        };

        // Test compression and decompression with BerachainHeader extension
        let buf = &mut Vec::new();
        berachain_header.compress_to_buf(buf);

        let decompressed = BerachainHeader::decompress(buf.as_slice()).unwrap();

        // Verify all fields match exactly, including the Berachain extension
        assert_eq!(decompressed, berachain_header);

        // Specifically verify the prev_proposer_pubkey field is preserved
        assert_eq!(decompressed.prev_proposer_pubkey, berachain_header.prev_proposer_pubkey);

        // Test with None prev_proposer_pubkey
        let mut berachain_header_no_pubkey = berachain_header.clone();
        berachain_header_no_pubkey.prev_proposer_pubkey = None;

        let buf_no_pubkey = &mut Vec::new();
        berachain_header_no_pubkey.compress_to_buf(buf_no_pubkey);

        let decompressed_no_pubkey = BerachainHeader::decompress(buf_no_pubkey.as_slice()).unwrap();

        assert_eq!(decompressed_no_pubkey, berachain_header_no_pubkey);
        assert_eq!(decompressed_no_pubkey.prev_proposer_pubkey, None);

        // Verify that headers with and without prev_proposer_pubkey produce different compressed
        // sizes This confirms the extension pattern is working correctly
        assert_ne!(
            buf.len(),
            buf_no_pubkey.len(),
            "Compressed sizes should differ when prev_proposer_pubkey is present vs absent"
        );
        assert!(
            buf.len() > buf_no_pubkey.len(),
            "Header with prev_proposer_pubkey should be larger when compressed"
        );
    }
}
