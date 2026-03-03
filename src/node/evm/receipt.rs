use crate::transaction::{BerachainTxEnvelope, BerachainTxType};
use reth_ethereum_primitives::Receipt;
use reth_evm::{
    Evm,
    eth::receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
};

#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct BerachainReceiptBuilder;

impl ReceiptBuilder for BerachainReceiptBuilder {
    type Transaction = BerachainTxEnvelope;
    type Receipt = Receipt<BerachainTxType>;

    fn build_receipt<E: Evm>(
        &self,
        ctx: ReceiptBuilderCtx<'_, BerachainTxType, E>,
    ) -> Self::Receipt {
        let ReceiptBuilderCtx { tx_type, result, cumulative_gas_used, .. } = ctx;
        Receipt {
            tx_type,
            success: result.is_success(),
            cumulative_gas_used,
            logs: result.into_logs(),
        }
    }
}
