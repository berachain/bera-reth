use alloy_consensus::{
    Signed, Transaction, TxEnvelope,
    crypto::RecoveryError,
    transaction::{Recovered, SignerRecoverable},
};
use alloy_eips::{
    Decodable2718, Encodable2718, Typed2718,
    eip2718::Eip2718Result,
    eip2930::AccessList,
    eip4844::{BlobTransactionValidationError, env_settings::KzgSettings},
    eip7594::BlobTransactionSidecarVariant,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{Address, B256, Bytes, ChainId, TxHash, TxKind, U256, bytes::BufMut};
use alloy_rlp::{Decodable, Encodable};
use jsonrpsee_core::Serialize;
use reth::{
    providers::errors::db::DatabaseError,
    revm::context::TxEnv,
    transaction_pool::{EthPoolTransaction, PoolTransaction},
};
use reth_db::table::{Compress, Decompress};
use reth_evm::{FromRecoveredTx, FromTxWithEncoded};
use reth_primitives_traits::{InMemorySize, SignedTransaction, serde_bincode_compat::RlpBincode};
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct PoLTx {
    #[serde(with = "alloy_serde::quantity", rename = "gas", alias = "gasLimit")]
    pub gas_limit: u64,
    pub to: Address,
    pub input: Bytes,
}
impl Transaction for PoLTx {
    fn chain_id(&self) -> Option<ChainId> {
        // Same as Op Deposit Tx
        None
    }

    fn nonce(&self) -> u64 {
        // Same as Op Deposit Tx
        0u64
    }

    fn gas_limit(&self) -> u64 {
        self.gas_limit
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
impl Encodable2718 for PoLTx {
    fn encode_2718_len(&self) -> usize {
        todo!()
    }

    fn encode_2718(&self, _out: &mut dyn BufMut) {
        todo!()
    }
}
impl Decodable2718 for PoLTx {
    fn typed_decode(_ty: u8, _buf: &mut &[u8]) -> Eip2718Result<Self> {
        todo!()
    }

    fn fallback_decode(_buf: &mut &[u8]) -> Eip2718Result<Self> {
        todo!()
    }
}
impl Typed2718 for PoLTx {
    fn ty(&self) -> u8 {
        todo!()
    }
}

impl Encodable for PoLTx {
    fn encode(&self, out: &mut dyn BufMut) {
        todo!()
    }
}

impl Decodable for PoLTx {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        todo!()
    }
}

impl InMemorySize for PoLTx {
    fn size(&self) -> usize {
        todo!()
    }
}

impl SignerRecoverable for PoLTx {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        todo!()
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        todo!()
    }
}

impl SignedTransaction for PoLTx {
    fn tx_hash(&self) -> &TxHash {
        // /Users/rezbera/Code/reth/crates/primitives-traits/src/transaction/signed.rs
        todo!()
    }
}

#[derive(Debug, Clone, alloy_consensus::TransactionEnvelope)]
#[envelope(tx_type_name = TxTypeCustom)]
#[allow(clippy::large_enum_variant)]
pub enum BerachainTxEnvelope {
    /// Existing Ethereum transactions (purely additive)
    #[envelope(flatten)]
    Ethereum(TxEnvelope),
    // /// Your 0-gas system transaction
    // #[envelope(ty = 190)] // equivalent to 0xBE
    // SystemRewards(PoLTx),
}

// impl Compress + Decompress + Serialize
impl Compress for BerachainTxEnvelope {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        todo!()
    }
}

impl Decompress for BerachainTxEnvelope {
    fn decompress(value: &[u8]) -> Result<Self, DatabaseError> {
        todo!()
    }
}

impl InMemorySize for BerachainTxEnvelope {
    fn size(&self) -> usize {
        todo!()
    }
}

impl SignerRecoverable for BerachainTxEnvelope {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        todo!()
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        todo!()
    }
}

impl SignedTransaction for BerachainTxEnvelope {
    fn tx_hash(&self) -> &TxHash {
        todo!()
    }
}

impl RlpBincode for BerachainTxEnvelope {}
impl RlpBincode for PoLTx {}

impl reth_codecs::Compact for BerachainTxEnvelope {
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

impl reth_codecs::Compact for PoLTx {
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

impl FromRecoveredTx<PoLTx> for TxEnv {
    fn from_recovered_tx(tx: &PoLTx, caller: Address) -> Self {
        todo!()
    }
}

impl FromTxWithEncoded<PoLTx> for TxEnv {
    fn from_encoded_tx(tx: &PoLTx, sender: Address, _encoded: Bytes) -> Self {
        todo!()
    }
}

impl FromRecoveredTx<BerachainTxEnvelope> for TxEnv {
    fn from_recovered_tx(tx: &BerachainTxEnvelope, sender: Address) -> Self {
        todo!()
    }
}

impl FromTxWithEncoded<BerachainTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &BerachainTxEnvelope, sender: Address, encoded: Bytes) -> Self {
        todo!()
    }
}

impl PoolTransaction for BerachainTxEnvelope {
    type TryFromConsensusError = Infallible;
    type Consensus = BerachainTxEnvelope;
    type Pooled = BerachainTxEnvelope;

    fn try_from_consensus(
        tx: Recovered<Self::Consensus>,
    ) -> Result<Self, Self::TryFromConsensusError> {
        Ok(tx.into_inner())
    }

    fn into_consensus(self) -> Recovered<Self::Consensus> {
        todo!("Convert to consensus transaction")
    }

    fn from_pooled(pooled: Recovered<Self::Pooled>) -> Self {
        pooled.into_inner()
    }

    fn try_into_pooled(self) -> Result<Recovered<Self::Pooled>, Self::TryFromConsensusError> {
        todo!("Convert to pooled transaction")
    }

    fn hash(&self) -> &TxHash {
        self.tx_hash()
    }

    fn sender(&self) -> Address {
        todo!("Implement sender recovery")
    }

    fn sender_ref(&self) -> &Address {
        todo!("Implement sender reference")
    }

    fn cost(&self) -> &U256 {
        todo!("Implement transaction cost calculation")
    }

    fn encoded_length(&self) -> usize {
        self.size()
    }
}

impl EthPoolTransaction for BerachainTxEnvelope {
    fn take_blob(&mut self) -> reth::transaction_pool::EthBlobTransactionSidecar {
        reth::transaction_pool::EthBlobTransactionSidecar::None
    }

    fn try_into_pooled_eip4844(
        self,
        _sidecar: Arc<BlobTransactionSidecarVariant>,
    ) -> Option<Recovered<Self::Pooled>> {
        None
    }

    fn try_from_eip4844(
        _tx: Recovered<Self::Consensus>,
        _sidecar: BlobTransactionSidecarVariant,
    ) -> Option<Self> {
        None
    }

    fn validate_blob(
        &self,
        _sidecar: &BlobTransactionSidecarVariant,
        _settings: &KzgSettings,
    ) -> Result<(), BlobTransactionValidationError> {
        Err(BlobTransactionValidationError::NotBlobTransaction(self.ty()))
    }
}

impl<T> From<Signed<T>> for BerachainTxEnvelope {
    fn from(value: Signed<T>) -> Self {
        todo!()
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
