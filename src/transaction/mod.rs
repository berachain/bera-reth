pub mod pol;
pub mod txtype;

/// Transaction type identifier for Berachain POL transactions
pub const POL_TX_TYPE: u8 = 126; // 0x7E

use alloy_consensus::{
    EthereumTxEnvelope, EthereumTypedTransaction, SignableTransaction, Signed, Transaction,
    TxEip4844, TxEip4844WithSidecar, TxEnvelope, TxType,
    crypto::RecoveryError,
    error::ValueError,
    transaction::{Recovered, SignerRecoverable},
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
use jsonrpsee_core::Serialize;
use reth::{providers::errors::db::DatabaseError, revm::context::TxEnv};
use reth_codecs::{
    Compact,
    alloy::transaction::{CompactEnvelope, Envelope, FromTxCompact, ToTxCompact},
};
use reth_db::table::{Compress, Decompress};
use reth_evm::{FromRecoveredTx, FromTxWithEncoded};
use reth_primitives_traits::{
    InMemorySize, MaybeSerde, SignedTransaction, serde_bincode_compat::RlpBincode,
};
use reth_rpc_convert::{SignTxRequestError, SignableTxRequest};
use serde::Deserialize;
use std::{hash::Hash, mem::size_of};

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

#[derive(Debug, Default, Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Compact)]
pub struct PoLTx {
    #[serde(with = "alloy_serde::quantity")]
    pub chain_id: ChainId,
    #[serde(skip)]
    pub from: Address, // system address - serde skip as from is derived from recover_signer in RPC
    pub to: Address,
    #[serde(with = "alloy_serde::quantity")]
    pub nonce: u64, // MUST be block_number - 1 for POL transactions per specification
    #[serde(with = "alloy_serde::quantity", rename = "gas", alias = "gasLimit")]
    pub gas_limit: u64,
    #[serde(with = "alloy_serde::quantity")]
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
        Some(self.gas_price)
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        None
    }

    fn priority_fee_or_price(&self) -> u128 {
        self.gas_price
    }

    fn effective_gas_price(&self, _base_fee: Option<u64>) -> u128 {
        0
    }

    fn is_dynamic_fee(&self) -> bool {
        false
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

        Ok(Self {
            chain_id: ChainId::decode(buf)?,
            from: Address::decode(buf)?,
            to: Address::decode(buf)?,
            nonce: u64::decode(buf)?,
            gas_limit: u64::decode(buf)?,
            gas_price: u128::decode(buf)?,
            input: Bytes::decode(buf)?,
        })
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
    Ethereum(TxEnvelope),
    /// Berachain PoL Transaction introduced in BRIP-0004
    #[envelope(ty = 126)] // POL_TX_TYPE - derive macro requires literal
    Berachain(Sealed<PoLTx>),
}

impl BerachainTxEnvelope {
    /// Returns the [`TxEip4844`] variant if the transaction is an EIP-4844 transaction.
    pub fn as_eip4844(&self) -> Option<Signed<TxEip4844>> {
        match self {
            Self::Ethereum(TxEnvelope::Eip4844(tx)) => {
                Some(tx.clone().map(|variant| variant.into()))
            }
            _ => None,
        }
    }
    pub fn tx_type(&self) -> BerachainTxType {
        match self {
            // Unwrap is safe here as berachain supports all eth tx types.
            Self::Ethereum(tx) => BerachainTxType::try_from(tx.tx_type() as u8).unwrap(),
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
                TxEnvelope::Eip4844(tx) => {
                    let (tx_variant, sig, hash) = tx.into_parts();
                    let tx_with_sidecar = match tx_variant {
                        alloy_consensus::TxEip4844Variant::TxEip4844(tx) => {
                            tx.with_sidecar(sidecar)
                        }
                        alloy_consensus::TxEip4844Variant::TxEip4844WithSidecar(
                            tx_with_sidecar,
                        ) => {
                            // If it already has a sidecar, replace it with the new one
                            let (base_tx, _old_sidecar) = tx_with_sidecar.into_parts();
                            base_tx.with_sidecar(sidecar)
                        }
                    };
                    let signed = Signed::new_unchecked(tx_with_sidecar, sig, hash);
                    Ok(EthereumTxEnvelope::Eip4844(signed))
                }
                _ => Err(ValueError::new_static(Self::Ethereum(tx), "Expected 4844 transaction")),
            },
            Self::Berachain(tx) => {
                Err(ValueError::new_static(Self::Berachain(tx), "Expected 4844 transaction"))
            }
        }
    }

    pub fn with_signer<T>(self, signer: Address) -> Recovered<Self> {
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
                // Delegate to the underlying Ethereum transaction compaction
                match tx {
                    TxEnvelope::Legacy(signed_tx) => {
                        signed_tx.tx().to_compact(buf);
                    }
                    TxEnvelope::Eip2930(signed_tx) => {
                        signed_tx.tx().to_compact(buf);
                    }
                    TxEnvelope::Eip1559(signed_tx) => {
                        signed_tx.tx().to_compact(buf);
                    }
                    TxEnvelope::Eip4844(signed_tx) => {
                        // Follow Reth's exact approach for EIP-4844 storage compatibility
                        // Reth doesn't use variant flags - it delegates to Alloy's compact
                        // implementation which handles TxEip4844Variant
                        // internally through the type system.
                        // See: reth/crates/storage/codecs/src/alloy/transaction/eip4844.rs:48-87
                        let tx_variant = signed_tx.tx();
                        match tx_variant {
                            alloy_consensus::TxEip4844Variant::TxEip4844(tx) => {
                                tx.to_compact(buf);
                            }
                            alloy_consensus::TxEip4844Variant::TxEip4844WithSidecar(tx) => {
                                // Store only the base transaction - sidecars handled separately
                                tx.tx().to_compact(buf);
                            }
                        }
                    }
                    TxEnvelope::Eip7702(signed_tx) => {
                        signed_tx.tx().to_compact(buf);
                    }
                }
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
                match eth_tx_type {
                    TxType::Legacy => {
                        let (tx, buf) = alloy_consensus::TxLegacy::from_compact(buf, buf.len());
                        let signed = Signed::new_unhashed(tx, signature);
                        (Self::Ethereum(TxEnvelope::Legacy(signed)), buf)
                    }
                    TxType::Eip2930 => {
                        let (tx, buf) = alloy_consensus::TxEip2930::from_compact(buf, buf.len());
                        let signed = Signed::new_unhashed(tx, signature);
                        (Self::Ethereum(TxEnvelope::Eip2930(signed)), buf)
                    }
                    TxType::Eip1559 => {
                        let (tx, buf) = alloy_consensus::TxEip1559::from_compact(buf, buf.len());
                        let signed = Signed::new_unhashed(tx, signature);
                        (Self::Ethereum(TxEnvelope::Eip1559(signed)), buf)
                    }
                    TxType::Eip4844 => {
                        // Follow Reth's exact EIP-4844 deserialization for storage compatibility
                        // Reth stores only the base TxEip4844 in compact form - no variant flags
                        // Sidecars are handled separately in pooled transactions
                        // See: reth/crates/storage/codecs/src/alloy/transaction/eip4844.rs:89-105
                        let (tx, remaining_buf) =
                            alloy_consensus::TxEip4844::from_compact(buf, buf.len());
                        let tx_variant = alloy_consensus::TxEip4844Variant::TxEip4844(tx);
                        let signed = Signed::new_unhashed(tx_variant, signature);
                        (Self::Ethereum(TxEnvelope::Eip4844(signed)), remaining_buf)
                    }
                    TxType::Eip7702 => {
                        let (tx, buf) = alloy_consensus::TxEip7702::from_compact(buf, buf.len());
                        let signed = Signed::new_unhashed(tx, signature);
                        (Self::Ethereum(TxEnvelope::Eip7702(signed)), buf)
                    }
                }
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
            Self::Ethereum(tx) => match tx {
                TxEnvelope::Legacy(signed_tx) => signed_tx.signature(),
                TxEnvelope::Eip2930(signed_tx) => signed_tx.signature(),
                TxEnvelope::Eip1559(signed_tx) => signed_tx.signature(),
                TxEnvelope::Eip4844(signed_tx) => signed_tx.signature(),
                TxEnvelope::Eip7702(signed_tx) => signed_tx.signature(),
            },
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

impl SignedTransaction for BerachainTxEnvelope
where
    Self: Clone + PartialEq + Eq + Decodable + Decodable2718 + MaybeSerde + InMemorySize,
{
    fn tx_hash(&self) -> &TxHash {
        match self {
            Self::Ethereum(tx) => tx.hash(),
            Self::Berachain(tx) => tx.hash_ref(),
        }
    }
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

impl From<reth_ethereum_primitives::TransactionSigned> for BerachainTxEnvelope {
    fn from(tx_signed: reth_ethereum_primitives::TransactionSigned) -> Self {
        // Convert to EthereumTxEnvelope first, then wrap in BerachainTxEnvelope
        let ethereum_tx: EthereumTxEnvelope<TxEip4844> = tx_signed;
        Self::Ethereum(ethereum_tx.into())
    }
}

impl From<EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>>
    for BerachainTxEnvelope
{
    fn from(
        ethereum_tx: EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>,
    ) -> Self {
        match ethereum_tx {
            EthereumTxEnvelope::Legacy(tx) => Self::Ethereum(TxEnvelope::Legacy(tx)),
            EthereumTxEnvelope::Eip2930(tx) => Self::Ethereum(TxEnvelope::Eip2930(tx)),
            EthereumTxEnvelope::Eip1559(tx) => Self::Ethereum(TxEnvelope::Eip1559(tx)),
            EthereumTxEnvelope::Eip4844(tx) => {
                // Convert the EIP-4844 transaction with sidecar to consensus format
                let (tx, sig, hash) = tx.into_parts();
                let (base_tx, _sidecar) = tx.into_parts();
                let consensus_tx = Signed::new_unchecked(base_tx, sig, hash);
                Self::Ethereum(TxEnvelope::Eip4844(
                    consensus_tx.map(alloy_consensus::TxEip4844Variant::TxEip4844),
                ))
            }
            EthereumTxEnvelope::Eip7702(tx) => Self::Ethereum(TxEnvelope::Eip7702(tx)),
        }
    }
}

impl TryFrom<BerachainTxEnvelope>
    for EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>
{
    type Error = TxConversionError;

    fn try_from(berachain_tx: BerachainTxEnvelope) -> Result<Self, Self::Error> {
        match berachain_tx {
            BerachainTxEnvelope::Ethereum(tx) => match tx {
                TxEnvelope::Legacy(tx) => Ok(EthereumTxEnvelope::Legacy(tx)),
                TxEnvelope::Eip2930(tx) => Ok(EthereumTxEnvelope::Eip2930(tx)),
                TxEnvelope::Eip1559(tx) => Ok(EthereumTxEnvelope::Eip1559(tx)),
                TxEnvelope::Eip4844(_tx) => {
                    // For consensus transactions without sidecars, we can't convert to pooled
                    // format This should only be called in contexts where we
                    // have the sidecar available
                    Err(TxConversionError::Eip4844MissingSidecar)
                }
                TxEnvelope::Eip7702(tx) => Ok(EthereumTxEnvelope::Eip7702(tx)),
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
                BerachainTxEnvelope::Ethereum(TxEnvelope::Legacy(tx.into_signed(signature)))
            }
            EthereumTypedTransaction::Eip2930(tx) => {
                BerachainTxEnvelope::Ethereum(TxEnvelope::Eip2930(tx.into_signed(signature)))
            }
            EthereumTypedTransaction::Eip1559(tx) => {
                BerachainTxEnvelope::Ethereum(TxEnvelope::Eip1559(tx.into_signed(signature)))
            }
            EthereumTypedTransaction::Eip4844(tx) => {
                BerachainTxEnvelope::Ethereum(TxEnvelope::Eip4844(
                    TxEip4844::from(tx)
                        .into_signed(signature)
                        .map(alloy_consensus::TxEip4844Variant::TxEip4844),
                ))
            }
            EthereumTypedTransaction::Eip7702(tx) => {
                BerachainTxEnvelope::Ethereum(TxEnvelope::Eip7702(tx.into_signed(signature)))
            }
        };
        Ok(signed)
    }
}

impl From<BerachainTxEnvelope> for EthereumTxEnvelope<alloy_consensus::TxEip4844Variant> {
    fn from(berachain_tx: BerachainTxEnvelope) -> Self {
        match berachain_tx {
            BerachainTxEnvelope::Ethereum(tx) => match tx {
                TxEnvelope::Legacy(tx) => EthereumTxEnvelope::Legacy(tx),
                TxEnvelope::Eip2930(tx) => EthereumTxEnvelope::Eip2930(tx),
                TxEnvelope::Eip1559(tx) => EthereumTxEnvelope::Eip1559(tx),
                TxEnvelope::Eip4844(tx) => EthereumTxEnvelope::Eip4844(tx),
                TxEnvelope::Eip7702(tx) => EthereumTxEnvelope::Eip7702(tx),
            },
            BerachainTxEnvelope::Berachain(_) => {
                // For now, we can't convert PoL transactions to Ethereum format
                // This should be handled at a higher level
                panic!("Cannot convert Berachain PoL transaction to Ethereum format")
            }
        }
    }
}

#[cfg(test)]
mod compact_envelope_tests {
    use super::*;
    use alloy_consensus::{TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy};
    use alloy_eips::{eip2930::AccessList, eip4844::Bytes48};
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
            BerachainTxEnvelope::Ethereum(TxEnvelope::Legacy(decoded_signed)) => {
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
            BerachainTxEnvelope::Ethereum(TxEnvelope::Eip1559(decoded_signed)) => {
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
            BerachainTxEnvelope::Ethereum(TxEnvelope::Eip4844(decoded_signed)) => {
                // Our BerachainTxEnvelope uses TxEip4844Variant, so extract the base transaction
                match decoded_signed.tx() {
                    alloy_consensus::TxEip4844Variant::TxEip4844(decoded_tx) => {
                        assert_eq!(decoded_tx, &eip4844_tx);
                        assert_eq!(decoded_signed.signature(), &signature);
                    }
                    _ => panic!("Expected base EIP-4844 variant"),
                }
            }
            _ => panic!("Expected Ethereum EIP-4844 transaction"),
        }
    }

    #[test]
    fn test_compact_envelope_roundtrip_eip4844_with_sidecar() {
        let berachain_envelope = create_eip4844_with_sidecar_berachain_envelope();

        // Encode using Berachain CompactEnvelope
        let mut buf = Vec::new();
        let len = CompactEnvelope::to_compact(&berachain_envelope, &mut buf);

        // Decode using Berachain CompactEnvelope
        let (decoded_envelope, _) =
            <BerachainTxEnvelope as CompactEnvelope>::from_compact(&buf, len);

        match decoded_envelope {
            BerachainTxEnvelope::Ethereum(TxEnvelope::Eip4844(decoded_signed)) => {
                // CompactEnvelope strips sidecars during serialization (they're not stored in DB)
                // so we expect to get back the base TxEip4844 variant, not the sidecar variant
                match decoded_signed.tx() {
                    alloy_consensus::TxEip4844Variant::TxEip4844(decoded_tx) => {
                        // Verify the base transaction fields are preserved
                        assert_eq!(decoded_tx.chain_id, ChainId::from(1u64));
                        assert_eq!(decoded_tx.nonce, 6);
                        assert_eq!(decoded_tx.gas_limit, 45_000);
                        assert_eq!(decoded_tx.to, Address::from([6u8; 20]));
                        assert_eq!(decoded_tx.value, U256::from(600));
                        assert_eq!(decoded_tx.input, Bytes::from("eip4844 with sidecar"));
                        assert_eq!(decoded_tx.blob_versioned_hashes, vec![B256::from([7u8; 32])]);
                        assert_eq!(decoded_tx.max_fee_per_blob_gas, 12_000_000_000u128);

                        // Verify signature is preserved
                        assert_eq!(decoded_signed.signature(), &create_test_signature());
                    }
                    variant => panic!(
                        "Expected base EIP-4844 variant (sidecar stripped during compact), got: {variant:?}"
                    ),
                }
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
                    BerachainTxEnvelope::Ethereum(TxEnvelope::Legacy(signed.clone()))
                }
                EthereumTxEnvelope::Eip2930(signed) => {
                    BerachainTxEnvelope::Ethereum(TxEnvelope::Eip2930(signed.clone()))
                }
                EthereumTxEnvelope::Eip1559(signed) => {
                    BerachainTxEnvelope::Ethereum(TxEnvelope::Eip1559(signed.clone()))
                }
                EthereumTxEnvelope::Eip4844(signed) => {
                    // Convert TxEip4844 to TxEip4844Variant for compatibility
                    let (tx, sig, hash) = signed.clone().into_parts();
                    let variant_signed = {
                        let variant = alloy_consensus::TxEip4844Variant::TxEip4844(tx);
                        alloy_consensus::Signed::new_unchecked(variant, sig, hash)
                    };
                    BerachainTxEnvelope::Ethereum(TxEnvelope::Eip4844(variant_signed))
                }
                EthereumTxEnvelope::Eip7702(signed) => {
                    BerachainTxEnvelope::Ethereum(TxEnvelope::Eip7702(signed.clone()))
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
                        TxEnvelope::Legacy(_) => 0u8,
                        TxEnvelope::Eip2930(_) => 1u8,
                        TxEnvelope::Eip1559(_) => 2u8,
                        TxEnvelope::Eip4844(_) => 3u8,
                        TxEnvelope::Eip7702(_) => 4u8,
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
            EthereumTxEnvelope::Legacy(signed) => TxEnvelope::Legacy(signed),
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

    fn create_eip4844_with_sidecar_berachain_envelope() -> BerachainTxEnvelope {
        use alloy_consensus::{TxEip4844Variant, TxEip4844WithSidecar};
        use alloy_eips::eip4844::{Blob, BlobTransactionSidecar};

        let base_tx = TxEip4844 {
            chain_id: ChainId::from(1u64),
            nonce: 6,
            gas_limit: 45_000,
            max_fee_per_gas: 65_000_000_000u128,
            max_priority_fee_per_gas: 3_500_000_000u128,
            to: Address::from([6u8; 20]),
            value: U256::from(600),
            access_list: AccessList::default(),
            blob_versioned_hashes: vec![B256::from([7u8; 32])],
            max_fee_per_blob_gas: 12_000_000_000u128,
            input: Bytes::from("eip4844 with sidecar"),
        };

        // Create a minimal sidecar for testing
        let blob = Blob::try_from([8u8; 131072].as_slice()).expect("Valid blob size");
        let sidecar = BlobTransactionSidecar {
            blobs: vec![blob],
            commitments: vec![Bytes48::from([9u8; 48])],
            proofs: vec![Bytes48::from([10u8; 48])],
        };

        let tx_with_sidecar = TxEip4844WithSidecar { tx: base_tx, sidecar };
        let variant = TxEip4844Variant::TxEip4844WithSidecar(tx_with_sidecar);

        let signed = Signed::new_unhashed(variant, create_test_signature());
        BerachainTxEnvelope::Ethereum(TxEnvelope::Eip4844(signed))
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
