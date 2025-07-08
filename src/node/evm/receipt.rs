use crate::transaction::BerachainTxEnvelope;
use alloy_consensus::{ReceiptEnvelope, TxType, transaction::TransactionMeta};
use alloy_eips::eip7840::BlobParams;
use alloy_rpc_types_eth::TransactionReceipt;
use reth_ethereum_primitives::{Receipt, TransactionSigned};
use reth_evm::{
    Evm,
    eth::receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
};
use reth_rpc_eth_types::{EthResult, receipt::build_receipt};

/// A builder that operates on Reth primitive types, specifically [`TransactionSigned`] and
/// [`Receipt`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct BerachainReceiptBuilder;

impl ReceiptBuilder for BerachainReceiptBuilder {
    type Transaction = BerachainTxEnvelope;
    type Receipt = Receipt;

    fn build_receipt<E: Evm>(
        &self,
        ctx: ReceiptBuilderCtx<'_, Self::Transaction, E>,
    ) -> Self::Receipt {
        let ReceiptBuilderCtx { tx, result, cumulative_gas_used, .. } = ctx;
        Receipt {
            tx_type: tx.tx_type().into(),
            // Success flag was added in `EIP-658: Embedding transaction status code in
            // receipts`.
            success: result.is_success(),
            cumulative_gas_used,
            logs: result.into_logs(),
        }
    }
}
