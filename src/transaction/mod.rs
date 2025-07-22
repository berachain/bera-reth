pub mod pol;
pub mod txtype;

/// Transaction type identifier for Berachain POL transactions
pub const POL_TX_TYPE: u8 = 126; // 0x7E

use alloy_consensus::{
    EthereumTxEnvelope, EthereumTypedTransaction, SignableTransaction, Signed, Transaction,
    TxEip4844, TxEip4844WithSidecar, TxEnvelope,
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
use reth_codecs::Compact;
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
    /// Existing Ethereum transactions (purely additive)
    #[envelope(flatten)]
    Ethereum(TxEnvelope),
    // /// Your 0-gas system transaction
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
            // TODO: Rez, is there a better way?
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
        // TODO: Rez sus
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
        match self {
            Self::Ethereum(tx) => {
                // Manually implement the compact encoding following the reth pattern
                buf.put_u8(tx.tx_type() as u8);
                match tx {
                    TxEnvelope::Legacy(signed_tx) => {
                        signed_tx.signature().to_compact(buf);
                        signed_tx.tx().to_compact(buf)
                    }
                    TxEnvelope::Eip2930(signed_tx) => {
                        signed_tx.signature().to_compact(buf);
                        signed_tx.tx().to_compact(buf)
                    }
                    TxEnvelope::Eip1559(signed_tx) => {
                        signed_tx.signature().to_compact(buf);
                        signed_tx.tx().to_compact(buf)
                    }
                    TxEnvelope::Eip4844(signed_tx) => {
                        signed_tx.signature().to_compact(buf);
                        // Handle TxEip4844Variant manually
                        let tx_variant = signed_tx.tx();
                        match tx_variant {
                            alloy_consensus::TxEip4844Variant::TxEip4844(tx) => {
                                buf.put_u8(0); // variant flag
                                tx.to_compact(buf)
                            }
                            alloy_consensus::TxEip4844Variant::TxEip4844WithSidecar(
                                tx_with_sidecar,
                            ) => {
                                buf.put_u8(1); // variant flag
                                let (base_tx, _sidecar) = tx_with_sidecar.clone().into_parts();
                                // For sidecars, we just store the base transaction
                                // The sidecar is handled separately in pooled transactions
                                base_tx.to_compact(buf)
                            }
                        }
                    }
                    TxEnvelope::Eip7702(signed_tx) => {
                        signed_tx.signature().to_compact(buf);
                        signed_tx.tx().to_compact(buf)
                    }
                }
            }
            Self::Berachain(tx) => {
                // For Berachain PoL transactions, encode the transaction type and the transaction
                buf.put_u8(u8::from(BerachainTxType::Berachain));
                tx.to_compact(buf)
            }
        }
    }

    fn from_compact(mut buf: &[u8], len: usize) -> (Self, &[u8]) {
        use alloy_consensus::{Signed, TxType};
        use alloy_primitives::bytes::Buf;

        let tx_type_byte = buf.get_u8();
        let tx_type = match tx_type_byte {
            0 => TxType::Legacy,
            1 => TxType::Eip2930,
            2 => TxType::Eip1559,
            3 => TxType::Eip4844,
            4 => TxType::Eip7702,
            POL_TX_TYPE => {
                // Handle Berachain PoL transaction
                let (pol_tx, remaining_buf) = PoLTx::from_compact(buf, len);
                return (Self::Berachain(Sealed::new(pol_tx)), remaining_buf);
            }
            _ => panic!("Unsupported BerachainTxEnvelope transaction type: {tx_type_byte}"),
        };

        let (signature, mut buf) = alloy_primitives::Signature::from_compact(buf, len);

        let (tx, remaining_buf) = match tx_type {
            TxType::Legacy => {
                let (tx, buf) = alloy_consensus::TxLegacy::from_compact(buf, len);
                let signed = Signed::new_unhashed(tx, signature);
                (TxEnvelope::Legacy(signed), buf)
            }
            TxType::Eip2930 => {
                let (tx, buf) = alloy_consensus::TxEip2930::from_compact(buf, len);
                let signed = Signed::new_unhashed(tx, signature);
                (TxEnvelope::Eip2930(signed), buf)
            }
            TxType::Eip1559 => {
                let (tx, buf) = alloy_consensus::TxEip1559::from_compact(buf, len);
                let signed = Signed::new_unhashed(tx, signature);
                (TxEnvelope::Eip1559(signed), buf)
            }
            TxType::Eip4844 => {
                // Handle TxEip4844Variant manually
                let variant_flag = buf.get_u8();
                let (tx_variant, buf) = match variant_flag {
                    0 => {
                        let (tx, buf) = alloy_consensus::TxEip4844::from_compact(buf, len);
                        (alloy_consensus::TxEip4844Variant::TxEip4844(tx), buf)
                    }
                    1 => {
                        // For sidecars, we just decode the base transaction
                        // The sidecar would be handled separately in pooled transactions
                        let (base_tx, buf) = alloy_consensus::TxEip4844::from_compact(buf, len);
                        (alloy_consensus::TxEip4844Variant::TxEip4844(base_tx), buf)
                    }
                    _ => panic!(
                        "Unsupported TxEip4844Variant flag in BerachainTxEnvelope: {variant_flag}"
                    ),
                };
                let signed = Signed::new_unhashed(tx_variant, signature);
                (TxEnvelope::Eip4844(signed), buf)
            }
            TxType::Eip7702 => {
                let (tx, buf) = alloy_consensus::TxEip7702::from_compact(buf, len);
                let signed = Signed::new_unhashed(tx, signature);
                (TxEnvelope::Eip7702(signed), buf)
            }
        };

        (Self::Ethereum(tx), remaining_buf)
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
