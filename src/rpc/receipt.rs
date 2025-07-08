use crate::transaction::BerachainTxEnvelope;
use alloy_consensus::transaction::TransactionMeta;
use alloy_eips::eip7840::BlobParams;
use alloy_rpc_types_eth::TransactionReceipt;
use reth_ethereum_primitives::{Receipt, TransactionSigned};
use reth_rpc_eth_types::{EthReceiptBuilder, EthResult};

pub struct BerachainEthReceiptBuilder {
    inner: EthReceiptBuilder,
}

impl BerachainEthReceiptBuilder {
    /// Returns a new builder with the base response body (L1 fields) set.
    ///
    /// Note: This requires _all_ block receipts because we need to calculate the gas used by the
    /// transaction.
    pub fn new(
        transaction: &BerachainTxEnvelope,
        meta: TransactionMeta,
        receipt: &Receipt,
        all_receipts: &[Receipt],
        blob_params: Option<BlobParams>,
    ) -> EthResult<Self> {
        match transaction {
            BerachainTxEnvelope::Ethereum(tx) => {
                let inner =
                    EthReceiptBuilder::new(tx.into(), meta, receipt, all_receipts, blob_params)?;
                Ok(Self { inner })
            }
        }
    }

    /// Builds a receipt response from the base response body, and any set additional fields.
    pub fn build(self) -> TransactionReceipt {
        self.inner.base
    }
}
