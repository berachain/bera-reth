use crate::transaction::{BerachainTxEnvelope, BerachainTxType};
use alloy_consensus::{
    ReceiptEnvelope,
    transaction::{Recovered, TransactionMeta},
};
use alloy_eips::eip7840::BlobParams;
use alloy_rpc_types_eth::TransactionReceipt;
use reth_ethereum_primitives::Receipt;
use reth_rpc_eth_types::receipt::build_receipt;

pub struct BerachainEthReceiptBuilder {
    base: TransactionReceipt,
}

impl BerachainEthReceiptBuilder {
    /// Returns a new builder with the base response body (L1 fields) set.
    ///
    /// Note: This requires _all_ block receipts because we need to calculate the gas used by the
    /// transaction.
    pub fn new(
        transaction: Recovered<&BerachainTxEnvelope>,
        meta: TransactionMeta,
        receipt: &Receipt,
        all_receipts: &[Receipt],
        blob_params: Option<BlobParams>,
    ) -> Self {
        let base = build_receipt(
            transaction,
            meta,
            receipt,
            all_receipts,
            blob_params,
            |receipt_with_bloom| {
                // Use the transaction type from the BerachainTxEnvelope to properly handle POL
                // transactions
                match transaction.as_ref() {
                    BerachainTxEnvelope::Ethereum(eth_tx) => match eth_tx.tx_type() {
                        alloy_consensus::TxType::Legacy => {
                            ReceiptEnvelope::Legacy(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip2930 => {
                            ReceiptEnvelope::Eip2930(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip1559 => {
                            ReceiptEnvelope::Eip1559(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip4844 => {
                            ReceiptEnvelope::Eip4844(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip7702 => {
                            ReceiptEnvelope::Eip7702(receipt_with_bloom)
                        }
                    },
                    BerachainTxEnvelope::Berachain(_) => {
                        // For POL transactions, use Legacy envelope format but with the correct
                        // type
                        ReceiptEnvelope::Legacy(receipt_with_bloom)
                    }
                }
            },
        );
        Self { base }
    }

    /// Builds a receipt response from the base response body, and any set additional fields.
    pub fn build(self) -> TransactionReceipt {
        self.base
    }
}
