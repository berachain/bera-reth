use super::{
    BerachainExecutionData, BerachainExecutionPayloadEnvelopeV4, BerachainExecutionPayloadSidecar,
};
use crate::{
    chainspec::BerachainChainSpec,
    primitives::{BerachainBlock, BerachainHeader, BerachainPrimitives, header::BlsPublicKey},
};
use alloy_consensus::BlockHeader;
use alloy_eips::{
    eip4895::Withdrawal,
    eip7685::{Requests, RequestsOrHash},
};
use alloy_primitives::{Address, B256, U256};
use alloy_rlp::Encodable;
use alloy_rpc_types::engine::{
    BlobsBundleV1, CancunPayloadFields, ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3,
    ExecutionPayloadEnvelopeV5, ExecutionPayloadFieldV2, ExecutionPayloadV1, ExecutionPayloadV3,
    PayloadId,
};
use reth::{
    api::{PayloadAttributes, PayloadTypes},
    builder::PayloadAttributesBuilder,
    chainspec::EthereumHardforks,
};
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_ethereum_engine_primitives::{BlobSidecars, BuiltPayloadConversionError};
use reth_node_ethereum::engine::EthPayloadAttributes;
use reth_payload_primitives::BuiltPayload;
use reth_primitives_traits::{NodePrimitives, SealedBlock, SealedHeader};
use std::sync::Arc;

/// Berachain-specific payload attributes
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainPayloadAttributes {
    #[serde(flatten)]
    pub inner: EthPayloadAttributes,
    #[serde(rename = "parentProposerPubKey")]
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
}

impl PayloadAttributes for BerachainPayloadAttributes {
    fn payload_id(&self, parent_hash: &B256) -> PayloadId {
        berachain_payload_id(parent_hash, self)
    }

    fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }
    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> {
        self.inner.withdrawals.as_ref()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.inner.parent_beacon_block_root
    }

    fn slot_number(&self) -> Option<u64> {
        self.inner.slot_number
    }

    fn target_gas_limit(&self) -> Option<u64> {
        self.inner.target_gas_limit
    }
}

impl BerachainPayloadAttributes {
    pub fn prev_proposer_pubkey(&self) -> Option<BlsPublicKey> {
        self.prev_proposer_pubkey
    }
}

/// Implementation for LocalPayloadAttributesBuilder to build BerachainPayloadAttributes
impl PayloadAttributesBuilder<BerachainPayloadAttributes, BerachainHeader>
    for LocalPayloadAttributesBuilder<BerachainChainSpec>
{
    fn build(&self, parent: &SealedHeader<BerachainHeader>) -> BerachainPayloadAttributes {
        let mut timestamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        if self.enforce_increasing_timestamp {
            timestamp = std::cmp::max(parent.timestamp().saturating_add(1), timestamp);
        }

        BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp,
                prev_randao: B256::random(),
                suggested_fee_recipient: Address::random(),
                withdrawals: self
                    .chain_spec
                    .is_shanghai_active_at_timestamp(timestamp)
                    .then(Default::default),
                parent_beacon_block_root: self
                    .chain_spec
                    .is_cancun_active_at_timestamp(timestamp)
                    .then(B256::random),
                ..Default::default()
            },
            prev_proposer_pubkey: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BerachainBuiltPayload {
    /// Identifier of the payload
    pub id: PayloadId,
    /// The built block
    pub block: Arc<SealedBlock<BerachainBlock>>,
    /// The fees of the block
    pub fees: U256,
    /// The blobs, proofs, and commitments in the block. If the block is pre-cancun, this will be
    /// empty.
    pub sidecars: BlobSidecars,
    /// The requests of the payload
    pub requests: Option<Requests>,
}

impl BerachainBuiltPayload {
    /// Initializes the payload with the given initial block
    ///
    /// Caution: This does not set any [`BlobSidecars`].
    pub const fn new(
        id: PayloadId,
        block: Arc<SealedBlock<BerachainBlock>>,
        fees: U256,
        requests: Option<Requests>,
    ) -> Self {
        Self { id, block, fees, requests, sidecars: BlobSidecars::Empty }
    }

    /// Sets blob transactions sidecars on the payload.
    pub fn with_sidecars(mut self, sidecars: impl Into<BlobSidecars>) -> Self {
        self.sidecars = sidecars.into();
        self
    }

    /// Try converting built payload into [`ExecutionPayloadEnvelopeV3`].
    ///
    /// Returns an error if the payload contains non EIP-4844 sidecar.
    pub fn try_into_v3(self) -> Result<ExecutionPayloadEnvelopeV3, BuiltPayloadConversionError> {
        let Self { block, fees, sidecars, .. } = self;

        let blobs_bundle = match sidecars {
            BlobSidecars::Empty => BlobsBundleV1::empty(),
            BlobSidecars::Eip4844(sidecars) => BlobsBundleV1::from(sidecars),
            BlobSidecars::Eip7594(_) => {
                return Err(BuiltPayloadConversionError::UnexpectedEip7594Sidecars);
            }
        };

        Ok(ExecutionPayloadEnvelopeV3 {
            execution_payload: ExecutionPayloadV3::from_block_unchecked(
                block.hash(),
                &Arc::unwrap_or_clone(block).into_block(),
            ),
            block_value: fees,
            // From the engine API spec:
            //
            // > Client software **MAY** use any heuristics to decide whether to set
            // `shouldOverrideBuilder` flag or not. If client software does not implement any
            // heuristic this flag **SHOULD** be set to `false`.
            //
            // Spec:
            // <https://github.com/ethereum/execution-apis/blob/fe8e13c288c592ec154ce25c534e26cb7ce0530d/src/engine/cancun.md#specification-2>
            should_override_builder: false,
            blobs_bundle,
        })
    }

    pub fn try_into_v4(
        self,
    ) -> Result<BerachainExecutionPayloadEnvelopeV4, BuiltPayloadConversionError> {
        let parent_proposer_pub_key = self.block.prev_proposer_pubkey;
        let requests = self.requests.clone().unwrap_or_default();
        Ok(BerachainExecutionPayloadEnvelopeV4 {
            execution_requests: requests,
            envelope_inner: self.try_into()?,
            parent_proposer_pub_key,
        })
    }
}

impl From<BerachainBuiltPayload> for ExecutionPayloadV1 {
    fn from(value: BerachainBuiltPayload) -> Self {
        Self::from_block_unchecked(
            value.block().hash(),
            &Arc::unwrap_or_clone(value.block).into_block(),
        )
    }
}

impl From<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV2 {
    fn from(value: BerachainBuiltPayload) -> Self {
        let BerachainBuiltPayload { block, fees, .. } = value;

        Self {
            block_value: fees,
            execution_payload: ExecutionPayloadFieldV2::from_block_unchecked(
                block.hash(),
                &Arc::unwrap_or_clone(block).into_block(),
            ),
        }
    }
}

impl TryFrom<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV3 {
    type Error = BuiltPayloadConversionError;

    fn try_from(value: BerachainBuiltPayload) -> Result<Self, Self::Error> {
        value.try_into_v3()
    }
}

impl TryFrom<BerachainBuiltPayload> for BerachainExecutionPayloadEnvelopeV4 {
    type Error = BuiltPayloadConversionError;

    fn try_from(value: BerachainBuiltPayload) -> Result<Self, Self::Error> {
        value.try_into_v4()
    }
}

/// Error returned when a [`BerachainBuiltPayload`] is converted into an
/// [`ExecutionPayloadEnvelopeV5`].
///
/// Berachain serves the Osaka payload via `engine_getPayloadV4P11`, so the V5
/// envelope is never produced. The trait bound on [`reth_engine_primitives::EngineTypes`]
/// requires the conversion to exist; this error makes the unsupported case
/// explicit instead of panicking.
#[derive(Debug, thiserror::Error)]
#[error("ExecutionPayloadEnvelopeV5 is not supported on Berachain; use engine_getPayloadV4P11")]
pub struct UnsupportedPayloadEnvelopeV5;

impl TryFrom<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV5 {
    type Error = UnsupportedPayloadEnvelopeV5;

    fn try_from(_value: BerachainBuiltPayload) -> Result<Self, Self::Error> {
        Err(UnsupportedPayloadEnvelopeV5)
    }
}

impl BuiltPayload for BerachainBuiltPayload {
    type Primitives = BerachainPrimitives;

    fn block(&self) -> &SealedBlock<<Self::Primitives as NodePrimitives>::Block> {
        &self.block
    }

    fn fees(&self) -> U256 {
        self.fees
    }

    fn requests(&self) -> Option<Requests> {
        self.requests.clone()
    }
}

impl From<BerachainBuiltPayload> for BerachainExecutionData {
    fn from(value: BerachainBuiltPayload) -> Self {
        let BerachainBuiltPayload { block, requests, .. } = value;
        let mut data = crate::engine::BerachainEngineTypes::block_to_payload(
            Arc::unwrap_or_clone(block),
            None,
        );

        // The sidecar derived from the block carries only the header's requests hash;
        // restore the request list stored on the built payload so downstream
        // validation sees the actual request bytes instead of skipping to the hash.
        if let Some(requests) = requests &&
            let Some(parent_beacon_block_root) = data.sidecar.parent_beacon_block_root()
        {
            let versioned_hashes = data.sidecar.versioned_hashes().cloned().unwrap_or_default();
            let parent_proposer_pub_key = data.sidecar.parent_proposer_pub_key();
            data.sidecar = BerachainExecutionPayloadSidecar::v4(
                CancunPayloadFields { parent_beacon_block_root, versioned_hashes },
                RequestsOrHash::Requests(requests),
                parent_proposer_pub_key,
            );
        }

        data
    }
}

/// Generates the payload id for Berachain payloads from the [`BerachainPayloadAttributes`].
///
/// This extends the standard Ethereum payload_id generation by including the
/// optional target_gas_limit and prev_proposer_pubkey in the hash calculation,
/// ensuring payload IDs are unique when either differs.
///
/// Returns an 8-byte identifier by hashing the payload components with sha256 hash.
pub fn berachain_payload_id(parent: &B256, attributes: &BerachainPayloadAttributes) -> PayloadId {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(parent.as_slice());
    hasher.update(&attributes.inner.timestamp.to_be_bytes()[..]);
    hasher.update(attributes.inner.prev_randao.as_slice());
    hasher.update(attributes.inner.suggested_fee_recipient.as_slice());

    if let Some(withdrawals) = &attributes.inner.withdrawals {
        let mut buf = Vec::new();
        withdrawals.encode(&mut buf);
        hasher.update(buf);
    }

    if let Some(parent_beacon_block) = attributes.inner.parent_beacon_block_root {
        hasher.update(parent_beacon_block);
    }

    // The gas limit steers block building, so it must be part of the ID or the
    // payload cache could serve a block built with a stale limit. The tag byte
    // discriminates presence from other optional fields; hashing nothing when
    // absent preserves the legacy IDs.
    if let Some(target_gas_limit) = attributes.inner.target_gas_limit {
        hasher.update([1u8]);
        hasher.update(target_gas_limit.to_be_bytes());
    }

    // Include prev_proposer_pubkey in the hash if present
    if let Some(proposer_pubkey) = attributes.prev_proposer_pubkey {
        hasher.update(proposer_pubkey);
    }

    let out = hasher.finalize();
    PayloadId::new(out[..8].try_into().expect("sufficient length"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::header::BlsPublicKey;
    use alloy_primitives::{Address, b256};
    use reth_node_ethereum::engine::EthPayloadAttributes;

    #[test]
    fn test_pubkey_affects_payload_id() {
        let parent = b256!("0000000000000000000000000000000000000000000000000000000000000001");

        let attributes_no_pubkey = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1000,
                prev_randao: b256!(
                    "0000000000000000000000000000000000000000000000000000000000000002"
                ),
                suggested_fee_recipient: Address::from([0x01; 20]),
                withdrawals: None,
                parent_beacon_block_root: None,
                ..Default::default()
            },
            prev_proposer_pubkey: None,
        };

        let attributes_with_pubkey = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1000,
                prev_randao: b256!(
                    "0000000000000000000000000000000000000000000000000000000000000002"
                ),
                suggested_fee_recipient: Address::from([0x01; 20]),
                withdrawals: None,
                parent_beacon_block_root: None,
                ..Default::default()
            },
            prev_proposer_pubkey: Some(BlsPublicKey::from([0x42; 48])),
        };

        // Test via PayloadAttributes::payload_id which calls berachain_payload_id
        let id_no_pubkey = attributes_no_pubkey.payload_id(&parent);
        let id_with_pubkey = attributes_with_pubkey.payload_id(&parent);

        // Critical test: presence of pubkey should affect payload ID
        assert_ne!(id_no_pubkey, id_with_pubkey);

        // Test different pubkeys produce different IDs
        let attributes_different_pubkey = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1000,
                prev_randao: b256!(
                    "0000000000000000000000000000000000000000000000000000000000000002"
                ),
                suggested_fee_recipient: Address::from([0x01; 20]),
                withdrawals: None,
                parent_beacon_block_root: None,
                ..Default::default()
            },
            prev_proposer_pubkey: Some(BlsPublicKey::from([0x43; 48])),
        };

        let id_different_pubkey = attributes_different_pubkey.payload_id(&parent);
        assert_ne!(id_with_pubkey, id_different_pubkey);
    }

    #[test]
    fn test_withdrawals_encoding_differences() {
        let parent = b256!("0000000000000000000000000000000000000000000000000000000000000001");

        let attributes_none = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1000,
                prev_randao: b256!(
                    "0000000000000000000000000000000000000000000000000000000000000002"
                ),
                suggested_fee_recipient: Address::from([0x01; 20]),
                withdrawals: None, // No withdrawals
                parent_beacon_block_root: None,
                ..Default::default()
            },
            prev_proposer_pubkey: None,
        };

        let attributes_empty = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1000,
                prev_randao: b256!(
                    "0000000000000000000000000000000000000000000000000000000000000002"
                ),
                suggested_fee_recipient: Address::from([0x01; 20]),
                withdrawals: Some(vec![]), // Empty withdrawals
                parent_beacon_block_root: None,
                ..Default::default()
            },
            prev_proposer_pubkey: None,
        };

        // Test via PayloadAttributes::payload_id which calls berachain_payload_id
        let id_none = attributes_none.payload_id(&parent);
        let id_empty = attributes_empty.payload_id(&parent);

        // Critical test: None vs Some([]) should produce different hashes
        // This matches geth behavior where None skips encoding, Some([]) encodes empty list
        assert_ne!(id_none, id_empty);
    }

    #[test]
    fn test_target_gas_limit_affects_payload_id() {
        let parent = b256!("0000000000000000000000000000000000000000000000000000000000000001");

        let base_inner = EthPayloadAttributes {
            timestamp: 1000,
            prev_randao: b256!("0000000000000000000000000000000000000000000000000000000000000002"),
            suggested_fee_recipient: Address::from([0x01; 20]),
            withdrawals: None,
            parent_beacon_block_root: None,
            ..Default::default()
        };

        let attributes_without =
            BerachainPayloadAttributes { inner: base_inner.clone(), prev_proposer_pubkey: None };
        let attributes_with = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                target_gas_limit: Some(30_000_000),
                ..base_inner.clone()
            },
            prev_proposer_pubkey: None,
        };
        let attributes_with_other = BerachainPayloadAttributes {
            inner: EthPayloadAttributes { target_gas_limit: Some(60_000_000), ..base_inner },
            prev_proposer_pubkey: None,
        };

        let id_without = attributes_without.payload_id(&parent);
        let id_with = attributes_with.payload_id(&parent);
        let id_with_other = attributes_with_other.payload_id(&parent);

        // Presence of a target gas limit must change the payload ID so the
        // cache cannot return a block built without the limit applied.
        assert_ne!(id_without, id_with);

        // Different limits must yield different IDs.
        assert_ne!(id_with, id_with_other);
    }

    #[test]
    fn test_payload_id_stable_when_target_gas_limit_absent() {
        // Golden value computed with the pre-target-gas-limit derivation:
        // sha256(parent ++ timestamp ++ prev_randao ++ fee_recipient)[..8].
        // Attributes without a target gas limit must keep producing legacy IDs.
        let parent = b256!("0000000000000000000000000000000000000000000000000000000000000001");
        let attributes = BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1000,
                prev_randao: b256!(
                    "0000000000000000000000000000000000000000000000000000000000000002"
                ),
                suggested_fee_recipient: Address::from([0x01; 20]),
                withdrawals: None,
                parent_beacon_block_root: None,
                ..Default::default()
            },
            prev_proposer_pubkey: None,
        };

        assert_eq!(
            attributes.payload_id(&parent),
            PayloadId::new([0x4a, 0x85, 0x13, 0xb9, 0x8d, 0xbf, 0xaf, 0x30])
        );
    }

    #[test]
    fn berachain_payload_attributes_serde() {
        // Test basic deserialization
        let json_basic = r#"{"timestamp":"0x1235","prevRandao":"0xf343b00e02dc34ec0124241f74f32191be28fb370bb48060f5fa4df99bda774c","suggestedFeeRecipient":"0xa94f5374fce5edbc8e2a8697c15331677e6ebf0b","withdrawals":null,"parentBeaconBlockRoot":null}"#;
        let attributes: BerachainPayloadAttributes = serde_json::from_str(json_basic).unwrap();
        assert_eq!(attributes.inner.timestamp, 0x1235);
        assert_eq!(attributes.prev_proposer_pubkey, None);

        // Test with proposer pubkey (Berachain-specific)
        let json_with_pubkey = r#"{"timestamp":"0x1235","prevRandao":"0xf343b00e02dc34ec0124241f74f32191be28fb370bb48060f5fa4df99bda774c","suggestedFeeRecipient":"0xa94f5374fce5edbc8e2a8697c15331677e6ebf0b","withdrawals":null,"parentBeaconBlockRoot":null,"parentProposerPubKey":"0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#;
        let attributes: BerachainPayloadAttributes =
            serde_json::from_str(json_with_pubkey).unwrap();
        assert!(attributes.prev_proposer_pubkey.is_some());
    }

    #[test]
    fn test_try_into_v4_propagates_pubkey_and_requests() {
        let pubkey = BlsPublicKey::from([0x42; 48]);
        let header = BerachainHeader {
            prev_proposer_pubkey: Some(pubkey),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            ..Default::default()
        };
        let block = alloy_consensus::Block {
            header,
            body: alloy_consensus::BlockBody {
                transactions: vec![],
                ommers: vec![],
                withdrawals: None,
            },
        };
        let sealed = SealedBlock::new_unhashed(block);

        let requests =
            Requests::new(vec![alloy_primitives::Bytes::from_static(b"\x00test_request")]);
        let payload = BerachainBuiltPayload::new(
            PayloadId::new([1; 8]),
            std::sync::Arc::new(sealed),
            U256::from(123),
            Some(requests.clone()),
        );

        let envelope = payload.try_into_v4().expect("conversion should succeed");

        assert_eq!(
            envelope.parent_proposer_pub_key,
            Some(pubkey),
            "parent_proposer_pub_key must match the source block header"
        );
        assert_eq!(
            envelope.execution_requests, requests,
            "execution_requests must be propagated from the built payload"
        );
    }

    #[test]
    fn test_try_into_v4_none_pubkey() {
        let header = BerachainHeader {
            prev_proposer_pubkey: None,
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            ..Default::default()
        };
        let block = alloy_consensus::Block {
            header,
            body: alloy_consensus::BlockBody {
                transactions: vec![],
                ommers: vec![],
                withdrawals: None,
            },
        };
        let sealed = SealedBlock::new_unhashed(block);

        let payload = BerachainBuiltPayload::new(
            PayloadId::new([2; 8]),
            std::sync::Arc::new(sealed),
            U256::ZERO,
            None,
        );

        let envelope = payload.try_into_v4().expect("conversion should succeed");

        assert_eq!(
            envelope.parent_proposer_pub_key, None,
            "parent_proposer_pub_key must be None when header has no pubkey"
        );
        assert!(
            envelope.execution_requests.is_empty(),
            "execution_requests must default to empty when payload has no requests"
        );
    }

    #[test]
    fn test_from_built_payload_preserves_requests() {
        let pubkey = BlsPublicKey::from([0x42; 48]);
        let requests =
            Requests::new(vec![alloy_primitives::Bytes::from_static(b"\x00test_request")]);
        let parent_beacon_block_root =
            b256!("0000000000000000000000000000000000000000000000000000000000000003");

        let header = BerachainHeader {
            prev_proposer_pubkey: Some(pubkey),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: Some(parent_beacon_block_root),
            requests_hash: Some(requests.requests_hash()),
            ..Default::default()
        };
        let block = alloy_consensus::Block {
            header,
            body: alloy_consensus::BlockBody {
                transactions: vec![],
                ommers: vec![],
                withdrawals: None,
            },
        };
        let sealed = SealedBlock::new_unhashed(block);

        let payload = BerachainBuiltPayload::new(
            PayloadId::new([4; 8]),
            std::sync::Arc::new(sealed),
            U256::ZERO,
            Some(requests.clone()),
        );

        let data = BerachainExecutionData::from(payload);

        assert_eq!(
            data.sidecar.requests(),
            Some(&requests),
            "converted sidecar must carry the request bytes, not just the requests hash"
        );
        assert_eq!(data.sidecar.parent_beacon_block_root(), Some(parent_beacon_block_root));
        assert_eq!(
            data.sidecar.parent_proposer_pub_key(),
            Some(pubkey),
            "proposer pubkey from the block must survive the sidecar rebuild"
        );
    }

    #[test]
    fn test_try_into_v5_returns_error_not_panic() {
        let header = BerachainHeader {
            prev_proposer_pubkey: None,
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            ..Default::default()
        };
        let block = alloy_consensus::Block {
            header,
            body: alloy_consensus::BlockBody {
                transactions: vec![],
                ommers: vec![],
                withdrawals: None,
            },
        };
        let sealed = SealedBlock::new_unhashed(block);
        let payload = BerachainBuiltPayload::new(
            PayloadId::new([3; 8]),
            std::sync::Arc::new(sealed),
            U256::ZERO,
            None,
        );

        let result: Result<ExecutionPayloadEnvelopeV5, UnsupportedPayloadEnvelopeV5> =
            payload.try_into();
        assert!(
            matches!(result, Err(UnsupportedPayloadEnvelopeV5)),
            "V5 conversion must return UnsupportedPayloadEnvelopeV5, never panic"
        );
    }
}
