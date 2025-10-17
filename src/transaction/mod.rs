pub mod pol;
pub mod rpc;
pub mod txtype;

use alloy_consensus::{
    EthereumTxEnvelope, EthereumTypedTransaction, SignableTransaction, Signed, Transaction,
    TxEip4844, TxEip4844WithSidecar,
    crypto::RecoveryError,
    error::ValueError,
    transaction::{Recovered, SignerRecoverable, TxHashRef},
};
use alloy_eips::{
    Decodable2718, Encodable2718, Typed2718, eip2718::Eip2718Result, eip2930::AccessList,
    eip7002::SYSTEM_ADDRESS, eip7594::BlobTransactionSidecarVariant, eip7702::SignedAuthorization,
};
use alloy_network::TxSigner;
use alloy_primitives::{
    Address, B256, Bytes, ChainId, Sealable, Sealed, Signature, TxHash, TxKind, U256,
    bytes::BufMut, keccak256,
};
use alloy_rlp::{Decodable, Encodable};
use alloy_rpc_types_eth::TransactionRequest;
use reth::{providers::errors::db::DatabaseError, revm::context::TxEnv};
use reth_codecs::{
    Compact,
    alloy::transaction::{CompactEnvelope, Envelope, FromTxCompact, ToTxCompact},
};
use reth_db::table::{Compress, Decompress};
use reth_ethereum_primitives::TransactionSigned;
use reth_evm::{FromRecoveredTx, FromTxWithEncoded};
use reth_primitives_traits::{
    InMemorySize, MaybeSerde, SignedTransaction, serde_bincode_compat::RlpBincode,
};
use reth_rpc_convert::{SignTxRequestError, SignableTxRequest};
use std::{hash::Hash, mem::size_of};

/// Transaction type identifier for Berachain POL transactions
pub const POL_TX_TYPE: u8 = 126; // 0x7E
pub const POL_TX_MAX_PRIORITY_FEE_PER_GAS: u128 = 0;

/// Error type for transaction conversion failures
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TxConversionError {
    /// Cannot convert EIP-4844 consensus transaction to pooled format without sidecar
    #[error("Cannot convert EIP-4844 consensus transaction to pooled format without sidecar")]
    Eip4844MissingSidecar,
    /// Cannot convert Berachain POL transaction to Ethereum format
    #[error("Cannot convert Berachain POL transaction to Ethereum format")]
    UnsupportedBerachainTransaction,
}

#[derive(Debug, Default, Clone, Hash, Eq, PartialEq, Compact)]
pub struct PoLTx {
    pub chain_id: ChainId,
    pub from: Address, // system address
    pub to: Address,
    pub nonce: u64, // MUST be block_number - 1 for POL transactions per specification
    pub gas_limit: u64,
    pub gas_price: u128, // gas_price to match Go struct
    pub input: Bytes,
}

impl Transaction for PoLTx {
    fn chain_id(&self) -> Option<ChainId> {
        Some(self.chain_id)
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }

    fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    fn gas_price(&self) -> Option<u128> {
        Some(self.gas_price)
    }

    fn max_fee_per_gas(&self) -> u128 {
        self.gas_price
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        Some(POL_TX_MAX_PRIORITY_FEE_PER_GAS)
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        None
    }

    fn priority_fee_or_price(&self) -> u128 {
        self.gas_price
    }

    fn effective_gas_price(&self, _base_fee: Option<u64>) -> u128 {
        self.gas_price
    }

    fn is_dynamic_fee(&self) -> bool {
        true
    }

    fn kind(&self) -> TxKind {
        TxKind::Call(self.to)
    }

    fn is_create(&self) -> bool {
        false
    }

    fn value(&self) -> U256 {
        U256::from(0)
    }

    fn input(&self) -> &Bytes {
        &self.input
    }

    fn access_list(&self) -> Option<&AccessList> {
        None
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        None
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        None
    }
}

impl PoLTx {
    fn tx_hash(&self) -> TxHash {
        let mut buf = Vec::with_capacity(self.encode_2718_len());
        self.encode_2718(&mut buf);
        keccak256(&buf)
    }

    fn rlp_payload_length(&self) -> usize {
        self.chain_id.length() +
            self.from.length() +
            self.to.length() +
            self.nonce.length() +
            self.gas_limit.length() +
            self.gas_price.length() +
            self.input.length()
    }

    fn rlp_encoded_length(&self) -> usize {
        let payload_length = self.rlp_payload_length();
        // Include RLP list header size
        alloy_rlp::Header { list: true, payload_length }.length() + payload_length
    }

    fn rlp_encode(&self, out: &mut dyn BufMut) {
        let payload_length = self.rlp_payload_length();

        alloy_rlp::Header { list: true, payload_length }.encode(out);
        self.chain_id.encode(out);
        self.from.encode(out);
        self.to.encode(out);
        self.nonce.encode(out);
        self.gas_limit.encode(out);
        self.gas_price.encode(out);
        self.input.encode(out);
    }

    fn rlp_decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let header = alloy_rlp::Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::UnexpectedString);
        }

        let remaining = buf.len();
        // Ensure payload is not shorter than indicated length
        if header.payload_length > remaining {
            return Err(alloy_rlp::Error::InputTooShort);
        }

        let decoded = Self {
            chain_id: ChainId::decode(buf)?,
            from: Address::decode(buf)?,
            to: Address::decode(buf)?,
            nonce: u64::decode(buf)?,
            gas_limit: u64::decode(buf)?,
            gas_price: u128::decode(buf)?,
            input: Bytes::decode(buf)?,
        };

        // Ensure indicated length matches decoded length
        if buf.len() + header.payload_length != remaining {
            return Err(alloy_rlp::Error::UnexpectedLength);
        };
        Ok(decoded)
    }
}

impl Encodable2718 for PoLTx {
    fn encode_2718_len(&self) -> usize {
        // 1 byte for transaction type + RLP encoded length
        1 + self.rlp_encoded_length()
    }

    fn encode_2718(&self, out: &mut dyn BufMut) {
        out.put_u8(self.ty());
        self.rlp_encode(out);
    }
}

impl Sealable for PoLTx {
    fn hash_slow(&self) -> B256 {
        self.tx_hash()
    }
}

impl Decodable2718 for PoLTx {
    fn typed_decode(ty: u8, buf: &mut &[u8]) -> Eip2718Result<Self> {
        if ty != u8::from(BerachainTxType::Berachain) {
            return Err(alloy_eips::eip2718::Eip2718Error::UnexpectedType(ty));
        }
        Self::rlp_decode(buf).map_err(Into::into)
    }

    fn fallback_decode(buf: &mut &[u8]) -> Eip2718Result<Self> {
        Self::rlp_decode(buf).map_err(Into::into)
    }
}

impl Typed2718 for PoLTx {
    fn ty(&self) -> u8 {
        u8::from(BerachainTxType::Berachain)
    }
}

impl Encodable for PoLTx {
    fn encode(&self, out: &mut dyn BufMut) {
        // Use consistent RLP list format
        self.rlp_encode(out);
    }
}

impl Decodable for PoLTx {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        // Use consistent RLP list format
        Self::rlp_decode(buf)
    }
}

impl InMemorySize for PoLTx {
    fn size(&self) -> usize {
        size_of::<Self>() + self.input.len()
    }
}

impl Compress for BerachainTxEnvelope {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        reth_codecs::Compact::to_compact(self, buf);
    }
}

impl Decompress for BerachainTxEnvelope {
    fn decompress(value: &[u8]) -> Result<Self, DatabaseError> {
        let (tx, _) = reth_codecs::Compact::from_compact(value, value.len());
        Ok(tx)
    }
}

impl SignerRecoverable for PoLTx {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        // POL transactions are always from the system address
        Ok(SYSTEM_ADDRESS)
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        Ok(SYSTEM_ADDRESS)
    }
}

#[derive(Debug, Clone, alloy_consensus::TransactionEnvelope)]
#[envelope(tx_type_name = BerachainTxType)]
#[allow(clippy::large_enum_variant)]
pub enum BerachainTxEnvelope {
    /// Existing Ethereum transactions
    #[envelope(flatten)]
    Ethereum(TransactionSigned),
    /// Berachain PoL Transaction introduced in BRIP-0004
    #[envelope(ty = 126)] // POL_TX_TYPE - derive macro requires literal
    Berachain(Sealed<PoLTx>),
}

impl BerachainTxEnvelope {
    /// Returns the [`TxEip4844`] variant if the transaction is an EIP-4844 transaction.
    pub fn as_eip4844(&self) -> Option<Signed<TxEip4844>> {
        match self {
            Self::Ethereum(EthereumTxEnvelope::Eip4844(tx)) => Some(tx.clone()),
            _ => None,
        }
    }
    pub fn tx_type(&self) -> BerachainTxType {
        match self {
            // Unwrap is safe here as berachain supports all eth tx types.
            Self::Ethereum(tx) => BerachainTxType::try_from(u8::from(tx.tx_type())).unwrap(),
            Self::Berachain(_) => BerachainTxType::Berachain,
        }
    }

    pub fn hash(&self) -> &TxHash {
        self.tx_hash()
    }
    /// Converts from an EIP-4844 transaction to a [`EthereumTxEnvelope<TxEip4844WithSidecar<T>>`]
    /// with the given sidecar.
    ///
    /// Returns an `Err` containing the original [`EthereumTxEnvelope`] if the transaction is not an
    /// EIP-4844 variant.
    pub fn try_into_pooled_eip4844<T>(
        self,
        sidecar: T,
    ) -> Result<EthereumTxEnvelope<TxEip4844WithSidecar<T>>, ValueError<Self>> {
        match self {
            Self::Ethereum(tx) => match tx {
                EthereumTxEnvelope::Eip4844(tx) => {
                    Ok(EthereumTxEnvelope::Eip4844(tx.map(|tx| tx.with_sidecar(sidecar))))
                }
                _ => Err(ValueError::new_static(Self::Ethereum(tx), "Expected 4844 transaction")),
            },
            Self::Berachain(tx) => {
                Err(ValueError::new_static(Self::Berachain(tx), "Expected 4844 transaction"))
            }
        }
    }

    pub fn with_signer(self, signer: Address) -> Recovered<Self> {
        Recovered::new_unchecked(self, signer)
    }
}

// STORAGE COMPATIBILITY: These CompactEnvelope implementations follow Reth's exact patterns
// to ensure database compatibility. Ethereum transactions use identical serialization to Reth.
// Only PoL transactions (type 126) use bera-reth specific encoding.
// See: reth/crates/storage/codecs/src/alloy/transaction/ethereum.rs for reference patterns
impl ToTxCompact for BerachainTxEnvelope {
    fn to_tx_compact(&self, buf: &mut (impl BufMut + AsMut<[u8]>)) {
        match self {
            Self::Ethereum(tx) => {
                // Delegate to TransactionSigned's implementation
                tx.to_tx_compact(buf);
            }
            Self::Berachain(signed_tx) => {
                // Serialize the PoL transaction directly
                signed_tx.as_ref().to_compact(buf);
            }
        }
    }
}

impl FromTxCompact for BerachainTxEnvelope {
    type TxType = BerachainTxType;

    fn from_tx_compact(buf: &[u8], tx_type: Self::TxType, signature: Signature) -> (Self, &[u8]) {
        match tx_type {
            BerachainTxType::Ethereum(eth_tx_type) => {
                // Delegate to TransactionSigned's implementation
                let (ethereum_tx, buf) =
                    TransactionSigned::from_tx_compact(buf, eth_tx_type, signature);
                (Self::Ethereum(ethereum_tx), buf)
            }
            BerachainTxType::Berachain => {
                // PoL transactions don't use real signatures - they use Sealed instead
                let (pol_tx, buf) = PoLTx::from_compact(buf, buf.len());
                let sealed = Sealed::new(pol_tx);
                (Self::Berachain(sealed), buf)
            }
        }
    }
}

impl Envelope for BerachainTxEnvelope {
    fn signature(&self) -> &Signature {
        match self {
            Self::Ethereum(tx) => tx.signature(),
            Self::Berachain(_) => {
                // PoL transactions don't have real signatures - use a zero signature
                static POL_SIGNATURE: Signature = Signature::new(U256::ZERO, U256::ZERO, false);
                &POL_SIGNATURE
            }
        }
    }

    fn tx_type(&self) -> Self::TxType {
        self.tx_type()
    }
}

impl InMemorySize for BerachainTxEnvelope {
    fn size(&self) -> usize {
        match self {
            Self::Ethereum(tx) => tx.size(),
            Self::Berachain(tx) => tx.size(),
        }
    }
}

impl SignerRecoverable for BerachainTxEnvelope {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        match self {
            Self::Ethereum(tx) => tx.recover_signer(),
            Self::Berachain(tx) => tx.recover_signer(),
        }
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        match self {
            Self::Ethereum(tx) => tx.recover_signer_unchecked(),
            Self::Berachain(tx) => tx.recover_signer_unchecked(),
        }
    }
}

impl TxHashRef for BerachainTxEnvelope {
    fn tx_hash(&self) -> &TxHash {
        match self {
            Self::Ethereum(tx) => tx.hash(),
            Self::Berachain(tx) => tx.hash_ref(),
        }
    }
}

impl SignedTransaction for BerachainTxEnvelope where
    Self: Clone + PartialEq + Eq + Decodable + Decodable2718 + MaybeSerde + InMemorySize
{
}

impl RlpBincode for BerachainTxEnvelope {}
impl RlpBincode for PoLTx {}

impl reth_codecs::Compact for BerachainTxEnvelope {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        CompactEnvelope::to_compact(self, buf)
    }

    fn from_compact(buf: &[u8], len: usize) -> (Self, &[u8]) {
        CompactEnvelope::from_compact(buf, len)
    }
}

impl FromRecoveredTx<PoLTx> for TxEnv {
    fn from_recovered_tx(tx: &PoLTx, caller: Address) -> Self {
        Self {
            tx_type: tx.ty(),
            caller,
            gas_limit: tx.gas_limit(),
            gas_price: tx.gas_price().unwrap_or_default(),
            kind: tx.kind(),
            value: tx.value(),
            data: tx.input.clone(),
            nonce: tx.nonce(),
            chain_id: None,
            access_list: Default::default(),
            gas_priority_fee: None,
            blob_hashes: vec![],
            max_fee_per_blob_gas: 0,
            authorization_list: vec![],
        }
    }
}

impl FromRecoveredTx<BerachainTxEnvelope> for TxEnv {
    fn from_recovered_tx(tx: &BerachainTxEnvelope, sender: Address) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(ethereum_tx) => {
                Self::from_recovered_tx(ethereum_tx, sender)
            }
            BerachainTxEnvelope::Berachain(berachain_tx) => {
                Self::from_recovered_tx(berachain_tx.inner(), sender)
            }
        }
    }
}

impl FromTxWithEncoded<BerachainTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &BerachainTxEnvelope, sender: Address, encoded: Bytes) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(ethereum_tx) => {
                TxEnv::from_encoded_tx(ethereum_tx, sender, encoded)
            }
            BerachainTxEnvelope::Berachain(berachain_tx) => TxEnv {
                tx_type: u8::from(BerachainTxType::Berachain),
                caller: SYSTEM_ADDRESS,
                gas_limit: berachain_tx.gas_limit(),
                gas_price: berachain_tx.gas_price().unwrap_or_default(),
                kind: berachain_tx.kind(),
                value: berachain_tx.value(),
                data: berachain_tx.input().clone(),
                nonce: berachain_tx.nonce(),
                chain_id: berachain_tx.chain_id(),
                access_list: AccessList(vec![]),
                gas_priority_fee: berachain_tx.max_priority_fee_per_gas(),
                blob_hashes: vec![],
                max_fee_per_blob_gas: 0,
                authorization_list: vec![],
            },
        }
    }
}

impl From<TransactionSigned> for BerachainTxEnvelope {
    fn from(tx_signed: TransactionSigned) -> Self {
        // Convert to EthereumTxEnvelope first, then wrap in BerachainTxEnvelope
        let ethereum_tx: EthereumTxEnvelope<TxEip4844> = tx_signed;
        Self::Ethereum(ethereum_tx)
    }
}

impl From<EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>>
    for BerachainTxEnvelope
{
    fn from(
        ethereum_tx: EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>,
    ) -> Self {
        Self::Ethereum(ethereum_tx.map_eip4844(|eip4844| eip4844.into()))
    }
}

impl TryFrom<BerachainTxEnvelope>
    for EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>
{
    type Error = TxConversionError;

    fn try_from(berachain_tx: BerachainTxEnvelope) -> Result<Self, Self::Error> {
        match berachain_tx {
            BerachainTxEnvelope::Ethereum(tx) => match tx {
                EthereumTxEnvelope::Legacy(tx) => Ok(EthereumTxEnvelope::Legacy(tx)),
                EthereumTxEnvelope::Eip2930(tx) => Ok(EthereumTxEnvelope::Eip2930(tx)),
                EthereumTxEnvelope::Eip1559(tx) => Ok(EthereumTxEnvelope::Eip1559(tx)),
                EthereumTxEnvelope::Eip4844(_tx) => {
                    // For consensus transactions without sidecars, we can't convert to pooled
                    // format This should only be called in contexts where we
                    // have the sidecar available
                    Err(TxConversionError::Eip4844MissingSidecar)
                }
                EthereumTxEnvelope::Eip7702(tx) => Ok(EthereumTxEnvelope::Eip7702(tx)),
            },
            BerachainTxEnvelope::Berachain(_) => {
                Err(TxConversionError::UnsupportedBerachainTransaction)
            }
        }
    }
}

impl SignableTxRequest<BerachainTxEnvelope> for TransactionRequest {
    async fn try_build_and_sign(
        self,
        signer: impl TxSigner<Signature> + Send,
    ) -> Result<BerachainTxEnvelope, SignTxRequestError> {
        let mut tx =
            self.build_typed_tx().map_err(|_| SignTxRequestError::InvalidTransactionRequest)?;
        let signature = signer.sign_transaction(&mut tx).await?;
        let signed = match tx {
            EthereumTypedTransaction::Legacy(tx) => {
                BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Legacy(tx.into_signed(signature)))
            }
            EthereumTypedTransaction::Eip2930(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip2930(tx.into_signed(signature)),
            ),
            EthereumTypedTransaction::Eip1559(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip1559(tx.into_signed(signature)),
            ),
            EthereumTypedTransaction::Eip4844(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip4844(TxEip4844::from(tx).into_signed(signature)),
            ),
            EthereumTypedTransaction::Eip7702(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip7702(tx.into_signed(signature)),
            ),
        };
        Ok(signed)
    }
}

/// Converts signed Ethereum typed transactions to BerachainTxEnvelope for simulation API
impl From<Signed<EthereumTypedTransaction<alloy_consensus::TxEip4844Variant>>>
    for BerachainTxEnvelope
{
    fn from(
        signed_tx: Signed<EthereumTypedTransaction<alloy_consensus::TxEip4844Variant>>,
    ) -> Self {
        use alloy_consensus::EthereumTypedTransaction;
        let (tx, signature, _hash) = signed_tx.into_parts();
        match tx {
            EthereumTypedTransaction::Legacy(tx) => {
                BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Legacy(tx.into_signed(signature)))
            }
            EthereumTypedTransaction::Eip2930(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip2930(tx.into_signed(signature)),
            ),
            EthereumTypedTransaction::Eip1559(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip1559(tx.into_signed(signature)),
            ),
            EthereumTypedTransaction::Eip4844(tx) => {
                BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip4844(
                    alloy_consensus::TxEip4844::from(tx).into_signed(signature),
                ))
            }
            EthereumTypedTransaction::Eip7702(tx) => BerachainTxEnvelope::Ethereum(
                EthereumTxEnvelope::Eip7702(tx.into_signed(signature)),
            ),
        }
    }
}

#[cfg(test)]
mod compact_envelope_tests {
    use super::*;
    use alloy_consensus::{TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy};
    use alloy_eips::eip2930::AccessList;
    use alloy_primitives::{Address, B256, Bytes, ChainId, TxKind, U256};
    use reth_codecs::alloy::transaction::CompactEnvelope;

    fn create_test_signature() -> Signature {
        Signature::new(U256::from(1u64), U256::from(2u64), false)
    }

    fn create_test_pol_tx() -> PoLTx {
        PoLTx {
            chain_id: ChainId::from(80084u64),
            from: Address::ZERO,
            to: Address::from([1u8; 20]),
            nonce: 42,
            gas_limit: 21000,
            gas_price: 1000000000u128,
            input: Bytes::from("test data"),
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_pol_to_pol() {
        let pol_tx = create_test_pol_tx();
        let envelope = BerachainTxEnvelope::Berachain(Sealed::new(pol_tx.clone()));

        // Encode using CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&envelope, &mut buf);

        // Decode using CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        match decoded_envelope {
            BerachainTxEnvelope::Berachain(decoded_pol) => {
                assert_eq!(decoded_pol.as_ref(), &pol_tx);
            }
            _ => panic!("Expected Berachain PoL transaction"),
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_ethereum_to_berachain_legacy() {
        let legacy_tx = TxLegacy {
            chain_id: Some(ChainId::from(1u64)),
            nonce: 10,
            gas_price: 20_000_000_000u128,
            gas_limit: 21_000,
            to: TxKind::Call(Address::from([1u8; 20])),
            value: U256::from(1000),
            input: Bytes::from("hello"),
        };

        let signature = create_test_signature();
        let signed_tx = Signed::new_unhashed(legacy_tx.clone(), signature);

        // Create Ethereum envelope
        let eth_envelope: EthereumTxEnvelope<TxEip4844> = EthereumTxEnvelope::Legacy(signed_tx);

        // Encode using Ethereum CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&eth_envelope, &mut buf);

        // Decode using Berachain CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        match decoded_envelope {
            BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Legacy(decoded_signed)) => {
                assert_eq!(decoded_signed.tx(), &legacy_tx);
                assert_eq!(decoded_signed.signature(), &signature);
            }
            _ => panic!("Expected Ethereum Legacy transaction"),
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_ethereum_to_berachain_eip1559() {
        let eip1559_tx = TxEip1559 {
            chain_id: ChainId::from(1u64),
            nonce: 5,
            gas_limit: 30_000,
            max_fee_per_gas: 50_000_000_000u128,
            max_priority_fee_per_gas: 2_000_000_000u128,
            to: TxKind::Call(Address::from([2u8; 20])),
            value: U256::from(2000),
            access_list: AccessList::default(),
            input: Bytes::from("eip1559 test"),
        };

        let signature = create_test_signature();
        let signed_tx = Signed::new_unhashed(eip1559_tx.clone(), signature);

        // Create Ethereum envelope
        let eth_envelope: EthereumTxEnvelope<TxEip4844> = EthereumTxEnvelope::Eip1559(signed_tx);

        // Encode using Ethereum CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&eth_envelope, &mut buf);

        // Decode using Berachain CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        match decoded_envelope {
            BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip1559(decoded_signed)) => {
                assert_eq!(decoded_signed.tx(), &eip1559_tx);
                assert_eq!(decoded_signed.signature(), &signature);
            }
            _ => panic!("Expected Ethereum EIP-1559 transaction"),
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_ethereum_to_berachain_eip4844() {
        let eip4844_tx = TxEip4844 {
            chain_id: ChainId::from(1u64),
            nonce: 7,
            gas_limit: 50_000,
            max_fee_per_gas: 100_000_000_000u128,
            max_priority_fee_per_gas: 5_000_000_000u128,
            to: Address::from([3u8; 20]),
            value: U256::from(3000),
            access_list: AccessList::default(),
            blob_versioned_hashes: vec![B256::from([4u8; 32])],
            max_fee_per_blob_gas: 10_000_000_000u128,
            input: Bytes::from("eip4844 test"),
        };

        let signature = create_test_signature();
        let signed_tx = Signed::new_unhashed(eip4844_tx.clone(), signature);

        // Create Ethereum envelope
        let eth_envelope: EthereumTxEnvelope<TxEip4844> = EthereumTxEnvelope::Eip4844(signed_tx);

        // Encode using Ethereum CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&eth_envelope, &mut buf);

        // Decode using Berachain CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        match decoded_envelope {
            BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip4844(decoded_signed)) => {
                // TransactionSigned uses TxEip4844 directly
                assert_eq!(decoded_signed.tx(), &eip4844_tx);
                assert_eq!(decoded_signed.signature(), &signature);
            }
            _ => panic!("Expected Ethereum EIP-4844 transaction"),
        }
    }

    #[test]
    fn test_compact_roundtrip_ethereum_to_berachain() {
        use reth_codecs::Compact;

        // Test that Ethereum transactions compacted by Ethereum Compact
        // can be decompacted by Berachain Compact for database compatibility
        let test_cases = vec![
            ("Legacy", create_legacy_envelope()),
            ("EIP-2930", create_eip2930_envelope()),
            ("EIP-1559", create_eip1559_envelope()),
            ("EIP-4844", create_eip4844_envelope()),
            ("EIP-7702", create_eip7702_envelope()),
        ];

        for (tx_name, eth_envelope) in test_cases {
            // Compact using Ethereum envelope (simulates Reth storage)
            let mut eth_buf = Vec::new();
            let eth_len = Compact::to_compact(&eth_envelope, &mut eth_buf);

            // Convert to BerachainTxEnvelope and compact using our implementation
            let berachain_envelope = match &eth_envelope {
                EthereumTxEnvelope::Legacy(signed) => {
                    BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Legacy(signed.clone()))
                }
                EthereumTxEnvelope::Eip2930(signed) => {
                    BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip2930(signed.clone()))
                }
                EthereumTxEnvelope::Eip1559(signed) => {
                    BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip1559(signed.clone()))
                }
                EthereumTxEnvelope::Eip4844(signed) => {
                    // Direct conversion since TransactionSigned uses TxEip4844
                    BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip4844(signed.clone()))
                }
                EthereumTxEnvelope::Eip7702(signed) => {
                    BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip7702(signed.clone()))
                }
            };

            let mut bera_buf = Vec::new();
            let bera_len = Compact::to_compact(&berachain_envelope, &mut bera_buf);

            // Verify the compacted content is identical
            assert_eq!(
                eth_buf, bera_buf,
                "{tx_name}: Compacted content must be identical for database compatibility"
            );
            assert_eq!(eth_len, bera_len, "{tx_name}: Compacted length must be identical");

            // Decompact using BerachainTxEnvelope (our implementation)
            let (decoded_envelope, _) =
                <BerachainTxEnvelope as CompactEnvelope>::from_compact(&eth_buf, eth_len);

            // Verify it decodes correctly as Ethereum transaction
            match decoded_envelope {
                BerachainTxEnvelope::Ethereum(decoded_tx) => {
                    // Verify transaction type matches
                    let original_type = match &eth_envelope {
                        EthereumTxEnvelope::Legacy(_) => 0u8,
                        EthereumTxEnvelope::Eip2930(_) => 1u8,
                        EthereumTxEnvelope::Eip1559(_) => 2u8,
                        EthereumTxEnvelope::Eip4844(_) => 3u8,
                        EthereumTxEnvelope::Eip7702(_) => 4u8,
                    };

                    let decoded_type = match &decoded_tx {
                        EthereumTxEnvelope::Legacy(_) => 0u8,
                        EthereumTxEnvelope::Eip2930(_) => 1u8,
                        EthereumTxEnvelope::Eip1559(_) => 2u8,
                        EthereumTxEnvelope::Eip4844(_) => 3u8,
                        EthereumTxEnvelope::Eip7702(_) => 4u8,
                    };

                    assert_eq!(
                        original_type, decoded_type,
                        "{tx_name}: Transaction type should be preserved"
                    );
                }
                BerachainTxEnvelope::Berachain(_) => {
                    panic!("{tx_name}: Should not decode as Berachain PoL transaction");
                }
            }
        }
    }

    #[test]
    fn test_compact_roundtrip_pol_to_pol() {
        use reth_codecs::Compact;

        let pol_tx = create_test_pol_tx();
        let berachain_envelope = BerachainTxEnvelope::Berachain(Sealed::new(pol_tx.clone()));

        // Compact using BerachainTxEnvelope
        let mut buf = Vec::new();
        let len = Compact::to_compact(&berachain_envelope, &mut buf);

        // Decompact using BerachainTxEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        // Verify the PoL transaction is preserved
        match decoded_envelope {
            BerachainTxEnvelope::Berachain(decoded_sealed) => {
                assert_eq!(
                    decoded_sealed.as_ref(),
                    &pol_tx,
                    "PoL transaction data should be preserved"
                );
            }
            _ => panic!("Should preserve Berachain PoL transaction format"),
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_all_ethereum_types() {
        // Test that all Ethereum transaction types can be encoded by Ethereum
        // and decoded by Berachain for full backwards compatibility

        // Legacy
        let legacy = create_legacy_envelope();
        test_compact_envelope_ethereum_to_berachain_roundtrip(legacy, "Legacy");

        // EIP-2930
        let eip2930 = create_eip2930_envelope();
        test_compact_envelope_ethereum_to_berachain_roundtrip(eip2930, "EIP-2930");

        // EIP-1559
        let eip1559 = create_eip1559_envelope();
        test_compact_envelope_ethereum_to_berachain_roundtrip(eip1559, "EIP-1559");

        // EIP-4844
        let eip4844 = create_eip4844_envelope();
        test_compact_envelope_ethereum_to_berachain_roundtrip(eip4844, "EIP-4844");

        // EIP-7702
        let eip7702 = create_eip7702_envelope();
        test_compact_envelope_ethereum_to_berachain_roundtrip(eip7702, "EIP-7702");
    }

    fn test_compact_envelope_ethereum_to_berachain_roundtrip(
        eth_envelope: EthereumTxEnvelope<TxEip4844>,
        tx_name: &str,
    ) {
        // Encode using Ethereum CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&eth_envelope, &mut buf);

        // Decode using Berachain CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        // Verify it's wrapped in Ethereum variant
        match decoded_envelope {
            BerachainTxEnvelope::Ethereum(_) => {
                // Success - we can decode Ethereum transactions
            }
            BerachainTxEnvelope::Berachain(_) => {
                panic!("{tx_name}: Should not decode as Berachain PoL transaction");
            }
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_pol_to_pol_comprehensive() {
        // Test that Berachain transactions can be encoded and decoded by Berachain
        let pol_tx = create_test_pol_tx();
        let berachain_envelope = BerachainTxEnvelope::Berachain(Sealed::new(pol_tx.clone()));

        // Encode using Berachain CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&berachain_envelope, &mut buf);

        // Decode using Berachain CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        match decoded_envelope {
            BerachainTxEnvelope::Berachain(decoded_pol) => {
                assert_eq!(decoded_pol.as_ref(), &pol_tx);
            }
            _ => panic!("Expected Berachain PoL transaction"),
        }
    }

    #[test]
    fn test_compact_envelope_storage_format_compatibility() {
        // Test that our CompactEnvelope format matches what Reth would produce
        // for Ethereum transactions (ensuring database compatibility)

        let legacy_tx = create_legacy_envelope();

        // Encode using Ethereum CompactEnvelope
        let mut eth_buf = Vec::new();
        let eth_len = CompactEnvelope::to_compact(&legacy_tx, &mut eth_buf);

        // Encode the same transaction wrapped in BerachainTxEnvelope
        let berachain_envelope = BerachainTxEnvelope::Ethereum(match legacy_tx.clone() {
            EthereumTxEnvelope::Legacy(signed) => EthereumTxEnvelope::Legacy(signed),
            _ => panic!("Expected legacy"),
        });

        let mut bera_buf = Vec::new();
        let bera_len = CompactEnvelope::to_compact(&berachain_envelope, &mut bera_buf);

        // The serialized format should be identical for storage compatibility
        assert_eq!(eth_buf, bera_buf, "Storage format must be identical for compatibility");
        assert_eq!(eth_len, bera_len, "Serialized length must be identical");
    }

    // Helper functions to create test envelopes
    fn create_legacy_envelope() -> EthereumTxEnvelope<TxEip4844> {
        let tx = TxLegacy {
            chain_id: Some(ChainId::from(1u64)),
            nonce: 1,
            gas_price: 20_000_000_000u128,
            gas_limit: 21_000,
            to: TxKind::Call(Address::from([1u8; 20])),
            value: U256::from(100),
            input: Bytes::new(),
        };
        let signed = Signed::new_unhashed(tx, create_test_signature());
        EthereumTxEnvelope::Legacy(signed)
    }

    fn create_eip2930_envelope() -> EthereumTxEnvelope<TxEip4844> {
        let tx = TxEip2930 {
            chain_id: ChainId::from(1u64),
            nonce: 2,
            gas_price: 25_000_000_000u128,
            gas_limit: 25_000,
            to: TxKind::Call(Address::from([2u8; 20])),
            value: U256::from(200),
            access_list: AccessList::default(),
            input: Bytes::new(),
        };
        let signed = Signed::new_unhashed(tx, create_test_signature());
        EthereumTxEnvelope::Eip2930(signed)
    }

    fn create_eip1559_envelope() -> EthereumTxEnvelope<TxEip4844> {
        let tx = TxEip1559 {
            chain_id: ChainId::from(1u64),
            nonce: 3,
            gas_limit: 30_000,
            max_fee_per_gas: 50_000_000_000u128,
            max_priority_fee_per_gas: 2_000_000_000u128,
            to: TxKind::Call(Address::from([3u8; 20])),
            value: U256::from(300),
            access_list: AccessList::default(),
            input: Bytes::new(),
        };
        let signed = Signed::new_unhashed(tx, create_test_signature());
        EthereumTxEnvelope::Eip1559(signed)
    }

    fn create_eip4844_envelope() -> EthereumTxEnvelope<TxEip4844> {
        let tx = TxEip4844 {
            chain_id: ChainId::from(1u64),
            nonce: 4,
            gas_limit: 40_000,
            max_fee_per_gas: 60_000_000_000u128,
            max_priority_fee_per_gas: 3_000_000_000u128,
            to: Address::from([4u8; 20]),
            value: U256::from(400),
            access_list: AccessList::default(),
            blob_versioned_hashes: vec![B256::from([5u8; 32])],
            max_fee_per_blob_gas: 15_000_000_000u128,
            input: Bytes::new(),
        };
        let signed = Signed::new_unhashed(tx, create_test_signature());
        EthereumTxEnvelope::Eip4844(signed)
    }

    fn create_eip7702_envelope() -> EthereumTxEnvelope<TxEip4844> {
        let tx = TxEip7702 {
            chain_id: ChainId::from(1u64),
            nonce: 5,
            gas_limit: 50_000,
            max_fee_per_gas: 70_000_000_000u128,
            max_priority_fee_per_gas: 4_000_000_000u128,
            to: Address::from([5u8; 20]),
            value: U256::from(500),
            access_list: AccessList::default(),
            authorization_list: vec![],
            input: Bytes::new(),
        };
        let signed = Signed::new_unhashed(tx, create_test_signature());
        EthereumTxEnvelope::Eip7702(signed)
    }
}

#[cfg(test)]
mod from_impl_tests {
    use super::*;
    use alloy_consensus::TxEip4844WithSidecar;
    use alloy_eips::eip4844::{Blob, BlobTransactionSidecar, Bytes48};

    #[test]
    fn test_from_ethereum_envelope_with_sidecar() {
        let base_tx = TxEip4844 {
            chain_id: ChainId::from(1u64),
            nonce: 1,
            gas_limit: 21_000,
            max_fee_per_gas: 20_000_000_000u128,
            max_priority_fee_per_gas: 1_000_000_000u128,
            to: Address::from([1u8; 20]),
            value: U256::from(100),
            access_list: AccessList::default(),
            blob_versioned_hashes: vec![B256::from([1u8; 32])],
            max_fee_per_blob_gas: 10_000_000_000u128,
            input: Bytes::new(),
        };

        let blob = Blob::try_from([0u8; 131072].as_slice()).unwrap();
        let sidecar = BlobTransactionSidecar {
            blobs: vec![blob],
            commitments: vec![Bytes48::from([0u8; 48])],
            proofs: vec![Bytes48::from([0u8; 48])],
        };

        let tx_with_sidecar = TxEip4844WithSidecar {
            tx: base_tx.clone(),
            sidecar: BlobTransactionSidecarVariant::from(sidecar),
        };

        let signed_tx = Signed::new_unhashed(tx_with_sidecar, create_test_signature());
        let ethereum_envelope: EthereumTxEnvelope<
            TxEip4844WithSidecar<BlobTransactionSidecarVariant>,
        > = EthereumTxEnvelope::Eip4844(signed_tx);

        let berachain_envelope = BerachainTxEnvelope::from(ethereum_envelope);

        match berachain_envelope {
            BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip4844(converted_tx)) => {
                assert_eq!(converted_tx.tx(), &base_tx);
            }
            _ => panic!("Expected EIP-4844 transaction"),
        }

        let legacy_signed =
            Signed::new_unhashed(alloy_consensus::TxLegacy::default(), create_test_signature());
        let ethereum_envelope: EthereumTxEnvelope<
            TxEip4844WithSidecar<BlobTransactionSidecarVariant>,
        > = EthereumTxEnvelope::Legacy(legacy_signed);

        let berachain_envelope = BerachainTxEnvelope::from(ethereum_envelope);

        match berachain_envelope {
            BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Legacy(_)) => {}
            _ => panic!("Expected Legacy transaction"),
        }
    }

    fn create_test_signature() -> Signature {
        Signature::new(U256::from(1u64), U256::from(2u64), false)
    }
}

#[cfg(test)]
mod pol_tx_rlp_tests {
    use super::*;
    use alloy_primitives::{Bytes, ChainId, address};
    use alloy_rlp::Header;

    fn create_test_pol_tx() -> PoLTx {
        PoLTx {
            chain_id: ChainId::from(80084u64),
            from: SYSTEM_ADDRESS,
            to: address!("4200000000000000000000000000000000000042"),
            nonce: 42,
            gas_limit: 21000,
            gas_price: 1000000000u128,
            input: Bytes::from("test data"),
        }
    }

    #[test]
    fn test_pol_tx_rlp_encode_decode_roundtrip() {
        let pol_tx = create_test_pol_tx();

        let mut encoded = Vec::new();
        pol_tx.rlp_encode(&mut encoded);

        let mut buf = encoded.as_slice();
        let decoded_pol_tx = PoLTx::rlp_decode(&mut buf).expect("Failed to decode PoLTx");

        assert_eq!(pol_tx, decoded_pol_tx);
    }

    #[test]
    fn test_pol_tx_rlp_encode_length() {
        let pol_tx = create_test_pol_tx();

        let mut encoded = Vec::new();
        pol_tx.rlp_encode(&mut encoded);

        assert_eq!(encoded.len(), pol_tx.rlp_encoded_length());
    }

    #[test]
    fn test_pol_tx_rlp_decode_invalid_header() {
        let invalid_data = vec![0x80]; // Empty RLP string, not list
        let mut buf = invalid_data.as_slice();

        let result = PoLTx::rlp_decode(&mut buf);
        assert!(matches!(result, Err(alloy_rlp::Error::UnexpectedString)));
    }

    #[test]
    fn test_pol_tx_rlp_decode_with_trailing_data() {
        // Tests valid RLP parsing with extra data after the transaction.
        // Extra data exists outside the RLP structure and should remain in buffer.
        let pol_tx = create_test_pol_tx();

        let mut encoded = Vec::new();
        pol_tx.rlp_encode(&mut encoded);
        encoded.extend_from_slice(&[0x42, 0x43, 0x44, 0x45]);

        let mut buf = encoded.as_slice();
        let decoded_pol_tx = PoLTx::rlp_decode(&mut buf).expect("Should decode successfully");

        assert_eq!(pol_tx, decoded_pol_tx);
        assert_eq!(buf, &[0x42, 0x43, 0x44, 0x45]);
    }

    #[test]
    fn test_pol_tx_rlp_decode_malformed_payload() {
        // Tests malicious RLP with garbage data inside the payload.
        // Header claims longer length than actual fields, embedding malicious data.
        let pol_tx = create_test_pol_tx();
        let mut payload = Vec::new();
        pol_tx.chain_id.encode(&mut payload);
        pol_tx.from.encode(&mut payload);
        pol_tx.to.encode(&mut payload);
        pol_tx.nonce.encode(&mut payload);
        pol_tx.gas_limit.encode(&mut payload);
        pol_tx.gas_price.encode(&mut payload);
        pol_tx.input.encode(&mut payload);
        payload.extend_from_slice(&[0xFF; 10]);

        let mut malformed_rlp = Vec::new();
        Header { list: true, payload_length: payload.len() }.encode(&mut malformed_rlp);
        malformed_rlp.extend_from_slice(&payload);

        let result = PoLTx::rlp_decode(&mut malformed_rlp.as_slice());
        assert!(matches!(result, Err(alloy_rlp::Error::UnexpectedLength)));
    }

    #[test]
    fn test_pol_tx_rlp_decode_insufficient_buffer() {
        let mut malformed_rlp = Vec::new();
        Header { list: true, payload_length: 1000 }.encode(&mut malformed_rlp);

        let result = PoLTx::rlp_decode(&mut malformed_rlp.as_slice());
        assert!(matches!(result, Err(alloy_rlp::Error::InputTooShort)));
    }
}
