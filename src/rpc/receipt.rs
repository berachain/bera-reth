use crate::transaction::{BerachainTxEnvelope, BerachainTxType};
use alloy_consensus::{
    Eip658Value, Receipt, ReceiptWithBloom, TxReceipt, TxType, Typed2718,
    transaction::{Recovered, TransactionMeta},
};
use alloy_eips::{
    eip2718::{Decodable2718, Eip2718Result, Encodable2718, IsTyped2718},
    eip7840::BlobParams,
};
use alloy_primitives::{Bloom, Log as PrimitiveLog};
use alloy_rlp::{BufMut, Decodable, Encodable};
use alloy_rpc_types_eth::TransactionReceipt;
use reth_ethereum_primitives::Receipt as RethReceipt;
use reth_primitives_traits::InMemorySize;
use reth_rpc_eth_types::receipt::build_receipt;
use std::borrow::Cow;

pub struct BerachainEthReceiptBuilder {
    base: TransactionReceipt<BerachainReceiptEnvelope>,
}

/// Minimal receipt envelope for Berachain transactions
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum BerachainReceiptEnvelope {
    #[serde(rename = "0x0")]
    Legacy(ReceiptWithBloom<Receipt<alloy_rpc_types_eth::Log>>),
    #[serde(rename = "0x1")]
    Eip2930(ReceiptWithBloom<Receipt<alloy_rpc_types_eth::Log>>),
    #[serde(rename = "0x2")]
    Eip1559(ReceiptWithBloom<Receipt<alloy_rpc_types_eth::Log>>),
    #[serde(rename = "0x3")]
    Eip4844(ReceiptWithBloom<Receipt<alloy_rpc_types_eth::Log>>),
    #[serde(rename = "0x4")]
    Eip7702(ReceiptWithBloom<Receipt<alloy_rpc_types_eth::Log>>),
    #[serde(rename = "0x7d")] // TODO: Change to 0x7e.
    Berachain(ReceiptWithBloom<Receipt<alloy_rpc_types_eth::Log>>),
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
            BerachainTxType::Berachain => 125u8, // POL transaction type
        }
    }
}

impl IsTyped2718 for BerachainReceiptEnvelope {
    fn is_type(type_id: u8) -> bool {
        matches!(type_id, 0 | 1 | 2 | 3 | 4 | 125)
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

impl BerachainEthReceiptBuilder {
    /// Returns a new builder with the base response body (L1 fields) set.
    ///
    /// Note: This requires _all_ block receipts because we need to calculate the gas used by the
    /// transaction.
    pub fn new(
        transaction: Recovered<&BerachainTxEnvelope>,
        meta: TransactionMeta,
        receipt: Cow<'_, reth_ethereum_primitives::Receipt<BerachainTxType>>,
        all_receipts: &[reth_ethereum_primitives::Receipt<BerachainTxType>],
        blob_params: Option<BlobParams>,
    ) -> Self {
        let tx_type = receipt.tx_type;

        let base = build_receipt(
            transaction,
            meta,
            receipt,
            all_receipts,
            blob_params,
            |receipt_with_bloom| {
                // Use the receipt's transaction type to properly handle all transaction types
                match tx_type {
                    BerachainTxType::Ethereum(eth_type) => match eth_type {
                        alloy_consensus::TxType::Legacy => {
                            BerachainReceiptEnvelope::Legacy(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip2930 => {
                            BerachainReceiptEnvelope::Eip2930(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip1559 => {
                            BerachainReceiptEnvelope::Eip1559(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip4844 => {
                            BerachainReceiptEnvelope::Eip4844(receipt_with_bloom)
                        }
                        alloy_consensus::TxType::Eip7702 => {
                            BerachainReceiptEnvelope::Eip7702(receipt_with_bloom)
                        }
                    },
                    BerachainTxType::Berachain => {
                        BerachainReceiptEnvelope::Berachain(receipt_with_bloom)
                    }
                }
            },
        );
        Self { base }
    }

    /// Builds a receipt response from the base response body, and any set additional fields.
    pub fn build(self) -> TransactionReceipt<BerachainReceiptEnvelope> {
        self.base
    }
}
