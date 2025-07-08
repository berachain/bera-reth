use alloy_consensus::{
    EthereumTxEnvelope, SignableTransaction, Signed, Transaction, TxEip4844, TxEip4844WithSidecar,
    TxEnvelope,
    crypto::RecoveryError,
    error::ValueError,
    transaction::{Recovered, RlpEcdsaEncodableTx, SignerRecoverable},
};
use alloy_eips::{
    Decodable2718, Encodable2718, Typed2718,
    eip2718::Eip2718Result,
    eip2930::AccessList,
    eip4844::{BlobTransactionValidationError, env_settings::KzgSettings},
    eip7594::BlobTransactionSidecarVariant,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{
    Address, B256, Bytes, ChainId, Signature, TxHash, TxKind, U256, bytes::BufMut,
};
use alloy_rlp::{Decodable, Encodable};
use jsonrpsee_core::Serialize;
use reth::{
    providers::errors::db::DatabaseError,
    revm::context::TxEnv,
    transaction_pool::{EthPoolTransaction, PoolTransaction},
};
use reth_db::table::{Compress, Decompress};
use reth_ethereum_primitives::PooledTransactionVariant;
use reth_evm::{FromRecoveredTx, FromTxWithEncoded};
use reth_primitives_traits::{
    InMemorySize, MaybeSerde, SignedTransaction, serde_bincode_compat::RlpBincode,
};
use reth_transaction_pool::{EthBlobTransactionSidecar, EthPooledTransaction};
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct PoLTx {
    pub chain_id: Option<ChainId>,
    pub nonce: u64,
    pub to: Address,
    pub data: Bytes,
}
impl Transaction for PoLTx {
    fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }

    fn gas_limit(&self) -> u64 {
        0u64 // No gas limit for system transactions
    }

    fn gas_price(&self) -> Option<u128> {
        None
    }

    fn max_fee_per_gas(&self) -> u128 {
        0
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        None
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        None
    }

    fn priority_fee_or_price(&self) -> u128 {
        0
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
        &self.data
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
impl Encodable2718 for PoLTx {
    fn encode_2718_len(&self) -> usize {
        use alloy_rlp::Encodable;
        let chain_id_len = match &self.chain_id {
            Some(id) => id.length(),
            None => 1, // Empty bytes
        };
        chain_id_len + self.nonce.length() + self.to.length() + self.data.length()
    }

    fn encode_2718(&self, out: &mut dyn BufMut) {
        use alloy_rlp::Encodable;
        match &self.chain_id {
            Some(id) => id.encode(out),
            None => {
                // Encode as empty bytes for None
                out.put_u8(0x80); // RLP encoding for empty string
            }
        }
        self.nonce.encode(out);
        self.to.encode(out);
        self.data.encode(out);
    }
}
impl Decodable2718 for PoLTx {
    fn typed_decode(ty: u8, buf: &mut &[u8]) -> Eip2718Result<Self> {
        if ty != 190u8 {
            return Err(alloy_eips::eip2718::Eip2718Error::UnexpectedType(ty));
        }
        Ok(Self::decode(buf)?)
    }

    fn fallback_decode(buf: &mut &[u8]) -> Eip2718Result<Self> {
        Ok(Self::decode(buf)?)
    }
}
impl Typed2718 for PoLTx {
    fn ty(&self) -> u8 {
        190u8 // 0xBE
    }
}

impl Encodable for PoLTx {
    fn encode(&self, out: &mut dyn BufMut) {
        // RLP encode the transaction fields as a list
        use alloy_rlp::{Encodable, Header};
        let mut buffer = Vec::new();

        match &self.chain_id {
            Some(id) => id.encode(&mut buffer),
            None => {
                // Encode as empty bytes for None
                buffer.push(0x80); // RLP encoding for empty string
            }
        }
        self.nonce.encode(&mut buffer);
        self.to.encode(&mut buffer);
        self.data.encode(&mut buffer);

        Header { list: true, payload_length: buffer.len() }.encode(out);
        out.put_slice(&buffer);
    }
}

impl Decodable for PoLTx {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        // RLP decode the transaction fields from a list
        use alloy_rlp::{Decodable, Header};
        let header = Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::Custom("expected list"));
        }

        // Decode chain_id manually
        let chain_id = if buf.is_empty() || buf[0] == 0x80 {
            // Empty bytes means None
            if !buf.is_empty() && buf[0] == 0x80 {
                *buf = &buf[1..]; // consume the empty byte
            }
            None
        } else {
            Some(ChainId::decode(buf)?)
        };

        let nonce = u64::decode(buf)?;
        let to = Address::decode(buf)?;
        let data = Bytes::decode(buf)?;

        Ok(PoLTx { chain_id, nonce, to, data })
    }
}

impl InMemorySize for PoLTx {
    fn size(&self) -> usize {
        std::mem::size_of::<Self>() + self.data.len()
    }
}

impl SignerRecoverable for PoLTx {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        // System transactions are pre-signed by the system
        Ok(Address::ZERO)
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        // System transactions are pre-signed by the system
        Ok(Address::ZERO)
    }
}

impl SignedTransaction for PoLTx {
    fn tx_hash(&self) -> &TxHash {
        // For system transactions, hash is deterministic based on content
        static ZERO_HASH: TxHash = TxHash::ZERO;
        &ZERO_HASH
    }
}

#[derive(Debug, Clone, alloy_consensus::TransactionEnvelope)]
#[envelope(tx_type_name = BerachainTxType)]
#[allow(clippy::large_enum_variant)]
pub enum BerachainTxEnvelope {
    /// Existing Ethereum transactions (purely additive)
    #[envelope(flatten)]
    Ethereum(TxEnvelope),
    /// Your 0-gas system transaction
    #[envelope(ty = 190)] // equivalent to 0xBE
    SystemRewards(PoLTx),
}

impl BerachainTxEnvelope {
    /// Returns the [`TxEip4844`] variant if the transaction is an EIP-4844 transaction.
    pub fn as_eip4844(&self) -> Option<Signed<TxEip4844>> {
        match self {
            Self::Ethereum(tx) => match tx {
                TxEnvelope::Eip4844(tx) => Some(tx.clone().map(|variant| variant.into())),
                _ => None,
            },
            Self::SystemRewards(_) => None,
        }
    }
    pub fn tx_type(&self) -> BerachainTxType {
        match self {
            Self::Ethereum(tx) => BerachainTxType::try_from(tx.tx_type() as u8).unwrap(),
            Self::SystemRewards(_) => BerachainTxType::try_from(190u8).unwrap(),
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
            Self::SystemRewards(_) => Err(ValueError::new_static(
                self,
                "SystemRewards transactions cannot be converted to pooled EIP-4844",
            )),
        }
    }

    pub fn with_signer<T>(self, signer: Address) -> Recovered<Self> {
        Recovered::new_unchecked(self, signer)
    }
}

impl Compress for BerachainTxEnvelope {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        // TODO: sus
        // Use the compact encoding for compression
        reth_codecs::Compact::to_compact(self, buf);
    }
}

impl Decompress for BerachainTxEnvelope {
    fn decompress(value: &[u8]) -> Result<Self, DatabaseError> {
        // TODO: sus
        // Use the compact decoding for decompression
        let (tx, _) = reth_codecs::Compact::from_compact(value, value.len());
        Ok(tx)
    }
}

impl InMemorySize for BerachainTxEnvelope {
    fn size(&self) -> usize {
        match self {
            Self::Ethereum(tx) => tx.size(),
            Self::SystemRewards(tx) => tx.size(),
        }
    }
}

impl SignerRecoverable for BerachainTxEnvelope {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        match self {
            Self::Ethereum(tx) => tx.recover_signer(),
            Self::SystemRewards(tx) => tx.recover_signer(),
        }
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        match self {
            Self::Ethereum(tx) => tx.recover_signer_unchecked(),
            Self::SystemRewards(tx) => tx.recover_signer_unchecked(),
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
            Self::SystemRewards(tx) => tx.tx_hash(),
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
        // TODO: @rez sus validity
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
                    _ => 0,
                }
            }
            Self::SystemRewards(tx) => {
                buf.put_u8(190u8); // 0xBE
                tx.to_compact(buf)
            }
        }
    }

    fn from_compact(mut buf: &[u8], len: usize) -> (Self, &[u8]) {
        use alloy_consensus::{Signed, TxType};
        use alloy_primitives::bytes::Buf;

        let tx_type_byte = buf.get_u8();
        if tx_type_byte == 190 {
            // SystemRewards transaction
            let (tx, remaining_buf) = PoLTx::from_compact(buf, len);
            return (Self::SystemRewards(tx), remaining_buf);
        }

        let tx_type = match tx_type_byte {
            0 => TxType::Legacy,
            1 => TxType::Eip2930,
            2 => TxType::Eip1559,
            3 => TxType::Eip4844,
            4 => TxType::Eip7702,
            _ => panic!("Unknown transaction type: {}", tx_type_byte),
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
                    _ => panic!("Invalid TxEip4844Variant flag: {}", variant_flag),
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

impl reth_codecs::Compact for PoLTx {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        let mut length = 0;
        length += self.chain_id.to_compact(buf);
        length += self.nonce.to_compact(buf);
        length += self.to.to_compact(buf);
        length += self.data.to_compact(buf);
        length
    }

    fn from_compact(buf: &[u8], len: usize) -> (Self, &[u8]) {
        let (chain_id, buf) = Option::<ChainId>::from_compact(buf, len);
        let (nonce, buf) = u64::from_compact(buf, len);
        let (to, buf) = Address::from_compact(buf, len);
        let (data, buf) = Bytes::from_compact(buf, len);

        (PoLTx { chain_id, nonce, to, data }, buf)
    }
}

impl FromRecoveredTx<PoLTx> for TxEnv {
    fn from_recovered_tx(tx: &PoLTx, caller: Address) -> Self {
        TxEnv {
            tx_type: tx.ty(),
            caller,
            gas_limit: 0u64,  // No gas limit for system transactions
            gas_price: 0u128, // No gas cost for system transactions
            gas_priority_fee: Some(0u128),
            kind: TxKind::Call(tx.to),
            value: U256::ZERO,
            nonce: tx.nonce,
            data: tx.data.clone(),
            chain_id: tx.chain_id,
            ..Default::default() // Use defaults for remaining fields
        }
    }
}

impl FromTxWithEncoded<PoLTx> for TxEnv {
    fn from_encoded_tx(tx: &PoLTx, sender: Address, _encoded: Bytes) -> Self {
        Self::from_recovered_tx(tx, sender)
    }
}

impl FromRecoveredTx<BerachainTxEnvelope> for TxEnv {
    fn from_recovered_tx(tx: &BerachainTxEnvelope, sender: Address) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(ethereum_tx) => {
                TxEnv::from_recovered_tx(ethereum_tx, sender)
            }
            BerachainTxEnvelope::SystemRewards(pol_tx) => TxEnv::from_recovered_tx(pol_tx, sender),
        }
    }
}

impl FromTxWithEncoded<BerachainTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &BerachainTxEnvelope, sender: Address, encoded: Bytes) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(ethereum_tx) => {
                TxEnv::from_encoded_tx(ethereum_tx, sender, encoded)
            }
            BerachainTxEnvelope::SystemRewards(pol_tx) => {
                TxEnv::from_encoded_tx(pol_tx, sender, encoded)
            }
        }
    }
}

impl From<BerachainTxType> for alloy_consensus::TxType {
    fn from(custom: BerachainTxType) -> Self {
        match u8::from(custom) {
            0 => Self::Legacy,
            1 => Self::Eip2930,
            2 => Self::Eip1559,
            3 => Self::Eip4844,
            4 => Self::Eip7702,
            190 => Self::Legacy, // SystemRewards -> fallback to Legacy for consensus
            _ => Self::Legacy,   // fallback for unknown types
        }
    }
}

impl<T> From<Signed<T>> for BerachainTxEnvelope {
    fn from(value: Signed<T>) -> Self {
        todo!()
    }
}

impl From<reth_ethereum_primitives::TransactionSigned> for BerachainTxEnvelope {
    fn from(tx_signed: reth_ethereum_primitives::TransactionSigned) -> Self {
        // Convert to EthereumTxEnvelope first, then wrap in BerachainTxEnvelope
        let ethereum_tx: EthereumTxEnvelope<TxEip4844> = tx_signed.into();
        Self::Ethereum(ethereum_tx.into())
    }
}

// Enable FromConsensusTx for transactions that can be converted
impl From<BerachainTxEnvelope>
    for alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844Variant>
{
    fn from(berachain_tx: BerachainTxEnvelope) -> Self {
        todo!()
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

impl From<BerachainTxEnvelope>
    for EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>
{
    fn from(berachain_tx: BerachainTxEnvelope) -> Self {
        match berachain_tx {
            BerachainTxEnvelope::Ethereum(tx) => match tx {
                TxEnvelope::Legacy(tx) => EthereumTxEnvelope::Legacy(tx),
                TxEnvelope::Eip2930(tx) => EthereumTxEnvelope::Eip2930(tx),
                TxEnvelope::Eip1559(tx) => EthereumTxEnvelope::Eip1559(tx),
                TxEnvelope::Eip4844(tx) => {
                    // For consensus transactions without sidecars, we can't convert to pooled
                    // format This should only be called in contexts where we
                    // have the sidecar available
                    panic!(
                        "Cannot convert EIP-4844 consensus transaction to pooled format without sidecar"
                    )
                }
                TxEnvelope::Eip7702(tx) => EthereumTxEnvelope::Eip7702(tx),
                _ => panic!("Unsupported transaction type"),
            },
            BerachainTxEnvelope::SystemRewards(_) => {
                panic!("System reward transactions cannot be converted to Ethereum format")
            }
        }
    }
}
