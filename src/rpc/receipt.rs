use crate::{
    primitives::BerachainPrimitives,
    transaction::{BerachainTxType, POL_TX_TYPE},
};
use alloy_consensus::{Eip658Value, Receipt, ReceiptWithBloom, TxReceipt, TxType, Typed2718};
use alloy_eips::eip2718::{Decodable2718, Eip2718Result, Encodable2718, IsTyped2718};
use alloy_primitives::Bloom;
use alloy_rlp::BufMut;
use alloy_rpc_types_eth::{Log, TransactionReceipt};
use reth_chainspec::EthChainSpec;
use reth_primitives_traits::InMemorySize;
use reth_rpc_convert::transaction::{ConvertReceiptInput, ReceiptConverter};
use reth_rpc_eth_types::{EthApiError, receipt::build_receipt};
use std::sync::Arc;

/// Minimal receipt envelope for Berachain transactions
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum BerachainReceiptEnvelope<T = Log> {
    #[serde(rename = "0x0")]
    Legacy(ReceiptWithBloom<Receipt<T>>),
    #[serde(rename = "0x1")]
    Eip2930(ReceiptWithBloom<Receipt<T>>),
    #[serde(rename = "0x2")]
    Eip1559(ReceiptWithBloom<Receipt<T>>),
    #[serde(rename = "0x3")]
    Eip4844(ReceiptWithBloom<Receipt<T>>),
    #[serde(rename = "0x4")]
    Eip7702(ReceiptWithBloom<Receipt<T>>),
    #[serde(rename = "0x7e")]
    Berachain(ReceiptWithBloom<Receipt<T>>),
}

impl BerachainReceiptEnvelope {
    pub fn from_typed<R>(tx_type: BerachainTxType, receipt: R) -> Self
    where
        R: Into<ReceiptWithBloom<Receipt<Log>>>,
    {
        match tx_type {
            BerachainTxType::Ethereum(tx_type) => match tx_type {
                TxType::Legacy => Self::Legacy(receipt.into()),
                TxType::Eip2930 => Self::Eip2930(receipt.into()),
                TxType::Eip1559 => Self::Eip1559(receipt.into()),
                TxType::Eip4844 => Self::Eip4844(receipt.into()),
                TxType::Eip7702 => Self::Eip7702(receipt.into()),
            },
            BerachainTxType::Berachain => Self::Berachain(receipt.into()),
        }
    }
}

impl BerachainReceiptEnvelope {
    /// Returns the transaction type of the receipt
    pub const fn tx_type(&self) -> BerachainTxType {
        match self {
            Self::Legacy(_) => BerachainTxType::Ethereum(TxType::Legacy),
            Self::Eip2930(_) => BerachainTxType::Ethereum(TxType::Eip2930),
            Self::Eip1559(_) => BerachainTxType::Ethereum(TxType::Eip1559),
            Self::Eip4844(_) => BerachainTxType::Ethereum(TxType::Eip4844),
            Self::Eip7702(_) => BerachainTxType::Ethereum(TxType::Eip7702),
            Self::Berachain(_) => BerachainTxType::Berachain,
        }
    }

    /// Returns inner receipt reference
    pub const fn as_receipt(&self) -> &Receipt<alloy_rpc_types_eth::Log> {
        match self {
            Self::Legacy(receipt) |
            Self::Eip2930(receipt) |
            Self::Eip1559(receipt) |
            Self::Eip4844(receipt) |
            Self::Eip7702(receipt) |
            Self::Berachain(receipt) => &receipt.receipt,
        }
    }

    /// Returns the bloom filter for this receipt
    pub const fn bloom(&self) -> &Bloom {
        match self {
            Self::Legacy(receipt) |
            Self::Eip2930(receipt) |
            Self::Eip1559(receipt) |
            Self::Eip4844(receipt) |
            Self::Eip7702(receipt) |
            Self::Berachain(receipt) => &receipt.logs_bloom,
        }
    }
}

impl TxReceipt for BerachainReceiptEnvelope {
    type Log = alloy_rpc_types_eth::Log;

    fn status_or_post_state(&self) -> Eip658Value {
        self.as_receipt().status_or_post_state()
    }

    fn status(&self) -> bool {
        self.as_receipt().status()
    }

    fn bloom(&self) -> Bloom {
        *self.bloom()
    }

    fn cumulative_gas_used(&self) -> u64 {
        self.as_receipt().cumulative_gas_used()
    }

    fn logs(&self) -> &[Self::Log] {
        self.as_receipt().logs()
    }
}

impl Typed2718 for BerachainReceiptEnvelope {
    fn ty(&self) -> u8 {
        match self.tx_type() {
            BerachainTxType::Ethereum(eth_type) => eth_type as u8,
            BerachainTxType::Berachain => POL_TX_TYPE, // POL transaction type
        }
    }
}

impl IsTyped2718 for BerachainReceiptEnvelope {
    fn is_type(type_id: u8) -> bool {
        matches!(type_id, 0 | 1 | 2 | 3 | 4 | POL_TX_TYPE)
    }
}

impl Encodable2718 for BerachainReceiptEnvelope {
    fn encode_2718_len(&self) -> usize {
        let ty = self.ty();
        (!matches!(ty, 0)) as usize + 64 // Approximate length, can be refined later
    }

    fn encode_2718(&self, out: &mut dyn BufMut) {
        let ty = self.ty();
        if !matches!(ty, 0) {
            out.put_u8(ty);
        }
        // For now, skip encoding - this will be implemented later if needed
    }
}

impl Decodable2718 for BerachainReceiptEnvelope {
    fn typed_decode(_ty: u8, _buf: &mut &[u8]) -> Eip2718Result<Self> {
        // For now, return an error - this will be implemented later if needed
        Err(alloy_eips::eip2718::Eip2718Error::UnexpectedType(_ty))
    }

    fn fallback_decode(_buf: &mut &[u8]) -> Eip2718Result<Self> {
        // For now, return an error - this will be implemented later if needed
        Err(alloy_eips::eip2718::Eip2718Error::UnexpectedType(0))
    }
}

impl InMemorySize for BerachainReceiptEnvelope {
    fn size(&self) -> usize {
        64 // Approximate size, can be refined later
    }
}

#[derive(Debug)]
pub struct BerachainEthReceiptConverter<ChainSpec> {
    chain_spec: Arc<ChainSpec>,
}

impl<ChainSpec> Clone for BerachainEthReceiptConverter<ChainSpec> {
    fn clone(&self) -> Self {
        Self { chain_spec: self.chain_spec.clone() }
    }
}

impl<ChainSpec> BerachainEthReceiptConverter<ChainSpec> {
    /// Creates a new converter with the given chain spec.
    pub const fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self { chain_spec }
    }
}

impl<ChainSpec> ReceiptConverter<BerachainPrimitives> for BerachainEthReceiptConverter<ChainSpec>
where
    ChainSpec: EthChainSpec + 'static,
{
    type RpcReceipt = TransactionReceipt<BerachainReceiptEnvelope>;
    type Error = EthApiError;

    fn convert_receipts(
        &self,
        inputs: Vec<ConvertReceiptInput<'_, BerachainPrimitives>>,
    ) -> Result<Vec<Self::RpcReceipt>, Self::Error> {
        let mut receipts = Vec::with_capacity(inputs.len());

        for input in inputs {
            let tx_type = input.receipt.tx_type;
            let blob_params = self.chain_spec.blob_params_at_timestamp(input.meta.timestamp);
            receipts.push(build_receipt(&input, blob_params, |receipt_with_bloom| {
                BerachainReceiptEnvelope::from_typed(tx_type, receipt_with_bloom)
            }));
        }

        Ok(receipts)
    }
}
