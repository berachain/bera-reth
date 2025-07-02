use alloy_consensus::{Transaction, TxEnvelope};
use alloy_eips::{
    Decodable2718, Encodable2718, Typed2718, eip2718::Eip2718Result, eip2930::AccessList,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{B256, Bytes, ChainId, TxKind, U256, bytes::BufMut};
use jsonrpsee_core::Serialize;
use serde::Deserialize;

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct PoLTx {}
impl Transaction for PoLTx {
    fn chain_id(&self) -> Option<ChainId> {
        todo!()
    }

    fn nonce(&self) -> u64 {
        todo!()
    }

    fn gas_limit(&self) -> u64 {
        todo!()
    }

    fn gas_price(&self) -> Option<u128> {
        todo!()
    }

    fn max_fee_per_gas(&self) -> u128 {
        todo!()
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        todo!()
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        todo!()
    }

    fn priority_fee_or_price(&self) -> u128 {
        todo!()
    }

    fn effective_gas_price(&self, base_fee: Option<u64>) -> u128 {
        todo!()
    }

    fn is_dynamic_fee(&self) -> bool {
        todo!()
    }

    fn kind(&self) -> TxKind {
        todo!()
    }

    fn is_create(&self) -> bool {
        todo!()
    }

    fn value(&self) -> U256 {
        todo!()
    }

    fn input(&self) -> &Bytes {
        todo!()
    }

    fn access_list(&self) -> Option<&AccessList> {
        todo!()
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        todo!()
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        todo!()
    }
}

impl Decodable2718 for PoLTx {
    fn typed_decode(ty: u8, buf: &mut &[u8]) -> Eip2718Result<Self> {
        todo!()
    }

    fn fallback_decode(buf: &mut &[u8]) -> Eip2718Result<Self> {
        todo!()
    }
}

impl Typed2718 for PoLTx {
    fn ty(&self) -> u8 {
        todo!()
    }
}

impl Encodable2718 for PoLTx {
    fn encode_2718_len(&self) -> usize {
        todo!()
    }

    fn encode_2718(&self, out: &mut dyn BufMut) {
        todo!()
    }
}

#[derive(Debug, Clone, alloy_consensus::TransactionEnvelope)]
#[envelope(tx_type_name = TxTypeCustom)]
pub enum BerachainTxEnvelope {
    /// Existing Ethereum transactions (purely additive)
    #[envelope(flatten)]
    Ethereum(TxEnvelope),

    /// Your 0-gas system transaction
    #[envelope(ty = 190)] // equivalent to 0xBE
    SystemRewards(PoLTx),
}
