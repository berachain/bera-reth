use alloy_consensus::{
    Transaction, TxEnvelope,
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
    revm::context::TxEnv,
    transaction_pool::{EthPoolTransaction, PoolTransaction},
};
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
        match tx {
            BerachainTxEnvelope::Ethereum(tx) => Self::from_recovered_tx(tx, sender),
            BerachainTxEnvelope::SystemRewards(tx) => Self::from_recovered_tx(tx, sender),
        }
    }
}

impl FromTxWithEncoded<BerachainTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &BerachainTxEnvelope, sender: Address, encoded: Bytes) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(tx) => Self::from_encoded_tx(tx, sender, encoded),
            BerachainTxEnvelope::SystemRewards(tx) => Self::from_encoded_tx(tx, sender, encoded),
        }
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

// Lossy conversion - only converts Ethereum transactions, fails for Berachain-specific ones
// impl TryFrom<BerachainTxEnvelope> for
// alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844Variant> {     type Error =
// &'static str;
//
//     fn try_from(berachain_tx: BerachainTxEnvelope) -> Result<Self, Self::Error> {
//         match berachain_tx {
//             BerachainTxEnvelope::Ethereum(eth_tx) => {
//                 // Convert TxEnvelope to EthereumTxEnvelope
//                 Ok(eth_tx.into())
//             }
//             BerachainTxEnvelope::SystemRewards(_) => {
//                 Err("Cannot convert Berachain SystemRewards transaction to EthereumTxEnvelope")
//             }
//         }
//     }
// }

// Enable FromConsensusTx for transactions that can be converted
impl From<BerachainTxEnvelope>
    for alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844Variant>
{
    fn from(berachain_tx: BerachainTxEnvelope) -> Self {
        match berachain_tx {
            BerachainTxEnvelope::Ethereum(eth_tx) => eth_tx.into(),
            BerachainTxEnvelope::SystemRewards(_) => {
                // This is a lossy conversion - we lose the SystemRewards transaction
                // In a real implementation, you might want to:
                // 1. Log a warning about losing transaction data
                // 2. Create a dummy Ethereum transaction
                // 3. Or panic if this case should never happen in your RPC layer
                todo!("Handle conversion of SystemRewards to Ethereum transaction")
            }
        }
    }
}
