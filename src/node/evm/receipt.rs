use crate::transaction::{BerachainTxEnvelope, BerachainTxType};
use reth_ethereum_primitives::Receipt;
use reth_evm::{
    Evm,
    eth::receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
};

/// A builder that operates on Reth primitive types, specifically `TransactionSigned` and
/// `Receipt`.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct BerachainReceiptBuilder;

impl ReceiptBuilder for BerachainReceiptBuilder {
    type Transaction = BerachainTxEnvelope;
    type Receipt = Receipt<BerachainTxType>;

    fn build_receipt<E: Evm>(
        &self,
        ctx: ReceiptBuilderCtx<'_, Self::Transaction, E>,
    ) -> Self::Receipt {
        let ReceiptBuilderCtx { tx, result, cumulative_gas_used, .. } = ctx;
        Receipt {
            tx_type: tx.tx_type(),
            // Success flag was added in `EIP-658: Embedding transaction status code in
            // receipts`.
            success: result.is_success(),
            cumulative_gas_used,
            logs: result.into_logs(),
        }
    }
}
