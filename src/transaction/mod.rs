use alloy_consensus::{
    Transaction, TxEnvelope, crypto::RecoveryError, transaction::SignerRecoverable,
};
use alloy_eips::{
    Decodable2718, Encodable2718, Typed2718, eip2718::Eip2718Result, eip2930::AccessList,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{Address, B256, Bytes, ChainId, TxHash, TxKind, U256, bytes::BufMut};
use alloy_rlp::{Decodable, Encodable};
use jsonrpsee_core::Serialize;
use reth_primitives_traits::{InMemorySize, SignedTransaction, serde_bincode_compat::RlpBincode};
use serde::Deserialize;

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

    /// Your 0-gas system transaction
    #[envelope(ty = 190)] // equivalent to 0xBE
    SystemRewards(PoLTx),
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
        match self {
            Self::Ethereum(tx) => tx.tx_hash(),
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
