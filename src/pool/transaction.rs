use crate::transaction::BerachainTxEnvelope;
use alloy_consensus::{
    EthereumTxEnvelope, Signed, Transaction, TxEip4844, TxEip4844WithSidecar,
    transaction::Recovered,
};
use alloy_eips::{
    Encodable2718, Typed2718,
    eip2930::AccessList,
    eip4844::{BlobTransactionValidationError, env_settings::KzgSettings},
    eip7594::BlobTransactionSidecarVariant,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{Address, B256, Bytes, TxHash, TxKind, U256};
use reth_ethereum_primitives::{PooledTransactionVariant, TransactionSigned};
use reth_primitives_traits::{InMemorySize, SignedTransaction};
use reth_transaction_pool::{EthBlobTransactionSidecar, EthPoolTransaction, PoolTransaction};
use std::{convert::Infallible, sync::Arc};

/// The default `PoolTransaction` for the Pool for Ethereum.
///
/// This type wraps a consensus transaction with additional cached data that's
/// frequently accessed by the pool for transaction ordering and validation:
///
/// - `cost`: Pre-calculated max cost (gas * price + value + blob costs)
/// - `encoded_length`: Cached RLP encoding length for size limits
/// - `blob_sidecar`: Blob data state (None/Missing/Present)
///
/// This avoids recalculating these values repeatedly during pool operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BerachainPooledTransaction {
    /// `EcRecovered` transaction, the consensus format.
    pub transaction: Recovered<TransactionSigned>,

    /// For EIP-1559 transactions: `max_fee_per_gas * gas_limit + tx_value`.
    /// For legacy transactions: `gas_price * gas_limit + tx_value`.
    /// For EIP-4844 blob transactions: `max_fee_per_gas * gas_limit + tx_value +
    /// max_blob_fee_per_gas * blob_gas_used`.
    pub cost: U256,

    /// This is the RLP length of the transaction, computed when the transaction is added to the
    /// pool.
    pub encoded_length: usize,

    /// The blob side car for this transaction
    pub blob_sidecar: EthBlobTransactionSidecar,
}

impl Typed2718 for BerachainPooledTransaction {
    fn ty(&self) -> u8 {
        self.transaction.ty()
    }
}

impl Transaction for BerachainPooledTransaction {
    fn chain_id(&self) -> Option<alloy_primitives::ChainId> {
        self.transaction.chain_id()
    }

    fn nonce(&self) -> u64 {
        self.transaction.nonce()
    }

    fn gas_limit(&self) -> u64 {
        self.transaction.gas_limit()
    }

    fn gas_price(&self) -> Option<u128> {
        self.transaction.gas_price()
    }

    fn max_fee_per_gas(&self) -> u128 {
        self.transaction.max_fee_per_gas()
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.transaction.max_priority_fee_per_gas()
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        self.transaction.max_fee_per_blob_gas()
    }

    fn priority_fee_or_price(&self) -> u128 {
        self.transaction.priority_fee_or_price()
    }

    fn effective_gas_price(&self, base_fee: Option<u64>) -> u128 {
        self.transaction.effective_gas_price(base_fee)
    }

    fn is_dynamic_fee(&self) -> bool {
        self.transaction.is_dynamic_fee()
    }

    fn kind(&self) -> TxKind {
        self.transaction.kind()
    }

    fn is_create(&self) -> bool {
        self.transaction.is_create()
    }

    fn value(&self) -> U256 {
        self.transaction.value()
    }

    fn input(&self) -> &Bytes {
        self.transaction.input()
    }

    fn access_list(&self) -> Option<&AccessList> {
        self.transaction.access_list()
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        self.transaction.blob_versioned_hashes()
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        self.transaction.authorization_list()
    }
}

/// A type alias for `PooledTransaction` that's also generic over blob sidecar.
pub type BerachainPooledTransactionVariant =
    alloy_consensus::EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>;

impl From<Recovered<BerachainPooledTransactionVariant>> for BerachainPooledTransaction {
    fn from(_value: Recovered<BerachainPooledTransactionVariant>) -> Self {
        todo!()
    }
}

impl BerachainPooledTransaction {
    /// Create new instance of [Self].
    ///
    /// Caution: In case of blob transactions, this does marks the blob sidecar as
    /// [`EthBlobTransactionSidecar::Missing`]
    pub fn new(
        transaction: Recovered<EthereumTxEnvelope<TxEip4844>>,
        encoded_length: usize,
    ) -> Self {
        let mut blob_sidecar = EthBlobTransactionSidecar::None;

        let gas_cost = U256::from(transaction.max_fee_per_gas())
            .saturating_mul(U256::from(transaction.gas_limit()));

        let mut cost = gas_cost.saturating_add(transaction.value());

        if let (Some(blob_gas_used), Some(max_fee_per_blob_gas)) =
            (transaction.blob_gas_used(), transaction.max_fee_per_blob_gas())
        {
            // Add max blob cost using saturating math to avoid overflow
            cost = cost.saturating_add(U256::from(
                max_fee_per_blob_gas.saturating_mul(blob_gas_used as u128),
            ));

            // because the blob sidecar is not included in this transaction variant, mark it as
            // missing
            blob_sidecar = EthBlobTransactionSidecar::Missing;
        }

        Self { transaction, cost, encoded_length, blob_sidecar }
    }

    /// Return the reference to the underlying transaction.
    pub const fn transaction(&self) -> &Recovered<TransactionSigned> {
        &self.transaction
    }
}

impl InMemorySize for BerachainPooledTransaction {
    fn size(&self) -> usize {
        self.transaction.size()
    }
}

impl PoolTransaction for BerachainPooledTransaction {
    type TryFromConsensusError = Infallible;

    type Consensus = BerachainTxEnvelope;

    type Pooled = BerachainPooledTransactionVariant;

    fn clone_into_consensus(&self) -> Recovered<Self::Consensus> {
        let (tx_signed, signer) = self.transaction().clone().into_parts();
        let berachain_tx = BerachainTxEnvelope::from(tx_signed);
        Recovered::new_unchecked(berachain_tx, signer)
    }

    fn into_consensus(self) -> Recovered<Self::Consensus> {
        let (tx_signed, signer) = self.transaction.into_parts();
        let berachain_tx = BerachainTxEnvelope::from(tx_signed);
        Recovered::new_unchecked(berachain_tx, signer)
    }

    fn from_pooled(tx: Recovered<Self::Pooled>) -> Self {
        let encoded_length = tx.encode_2718_len();
        let (tx, signer) = tx.into_parts();
        match tx {
            PooledTransactionVariant::Eip4844(tx) => {
                // include the blob sidecar
                let (tx, sig, hash) = tx.into_parts();
                let (tx, blob) = tx.into_parts();
                let tx = Signed::new_unchecked(tx, sig, hash);
                let tx = TransactionSigned::from(tx);
                let tx = Recovered::new_unchecked(tx, signer);
                let mut pooled = Self::new(tx, encoded_length);
                pooled.blob_sidecar = EthBlobTransactionSidecar::Present(blob);
                pooled
            }
            tx => {
                // no blob sidecar
                let tx = Recovered::new_unchecked(tx.into(), signer);
                Self::new(tx, encoded_length)
            }
        }
    }

    /// Returns hash of the transaction.
    fn hash(&self) -> &TxHash {
        self.transaction.tx_hash()
    }

    /// Returns the Sender of the transaction.
    fn sender(&self) -> Address {
        self.transaction.signer()
    }

    /// Returns a reference to the Sender of the transaction.
    fn sender_ref(&self) -> &Address {
        self.transaction.signer_ref()
    }

    /// Returns the cost that this transaction is allowed to consume:
    ///
    /// For EIP-1559 transactions: `max_fee_per_gas * gas_limit + tx_value`.
    /// For legacy transactions: `gas_price * gas_limit + tx_value`.
    /// For EIP-4844 blob transactions: `max_fee_per_gas * gas_limit + tx_value +
    /// max_blob_fee_per_gas * blob_gas_used`.
    fn cost(&self) -> &U256 {
        &self.cost
    }

    /// Returns the length of the rlp encoded object
    fn encoded_length(&self) -> usize {
        self.encoded_length
    }
}

impl EthPoolTransaction for BerachainPooledTransaction {
    fn take_blob(&mut self) -> EthBlobTransactionSidecar {
        if self.is_eip4844() {
            std::mem::replace(&mut self.blob_sidecar, EthBlobTransactionSidecar::Missing)
        } else {
            EthBlobTransactionSidecar::None
        }
    }

    fn try_into_pooled_eip4844(
        self,
        sidecar: Arc<BlobTransactionSidecarVariant>,
    ) -> Option<Recovered<Self::Pooled>> {
        let (signed_transaction, signer) = self.into_consensus().into_parts();
        let pooled_transaction =
            signed_transaction.try_into_pooled_eip4844(Arc::unwrap_or_clone(sidecar)).ok()?;

        Some(Recovered::new_unchecked(pooled_transaction, signer))
    }

    fn try_from_eip4844(
        tx: Recovered<Self::Consensus>,
        sidecar: BlobTransactionSidecarVariant,
    ) -> Option<Self> {
        let (tx, signer) = tx.into_parts();
        tx.try_into_pooled_eip4844(sidecar)
            .ok()
            .map(|tx| tx.with_signer(signer))
            .map(Self::from_pooled)
    }

    fn validate_blob(
        &self,
        sidecar: &BlobTransactionSidecarVariant,
        settings: &KzgSettings,
    ) -> Result<(), BlobTransactionValidationError> {
        match self.transaction.inner().as_eip4844() {
            Some(tx) => tx.tx().validate_blob(sidecar, settings),
            _ => Err(BlobTransactionValidationError::NotBlobTransaction(self.ty())),
        }
    }
}
