use crate::transaction::BerachainTxEnvelope;
use alloy_primitives::{Address, Bytes};
use reth::revm::context::TxEnv;
use reth_evm::{FromRecoveredTx, FromTxWithEncoded};

impl FromRecoveredTx<BerachainTxEnvelope> for TxEnv {
    fn from_recovered_tx(tx: &BerachainTxEnvelope, sender: Address) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(tx) => Self::from_recovered_tx(tx, sender),
            BerachainTxEnvelope::SystemRewards(tx) => todo!(),
        }
    }
}

impl FromTxWithEncoded<BerachainTxEnvelope> for TxEnv {
    fn from_encoded_tx(tx: &BerachainTxEnvelope, sender: Address, encoded: Bytes) -> Self {
        match tx {
            BerachainTxEnvelope::Ethereum(tx) => Self::from_encoded_tx(tx, sender, encoded),
            BerachainTxEnvelope::SystemRewards(tx) => todo!(),
        }
    }
}
