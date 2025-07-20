//! Compact implementation for [`BerachainTxType`]

use crate::transaction::{BerachainTxType, POL_TX_TYPE};
use alloy_consensus::TxType;
use bytes::{Buf, BufMut};
use reth::providers::errors::db::DatabaseError;
use reth_codecs::{Compact, txtype::COMPACT_EXTENDED_IDENTIFIER_FLAG};
use reth_db_api::table::{Compress, Decompress};
use reth_primitives_traits::InMemorySize;

impl Compact for BerachainTxType {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: bytes::BufMut + AsMut<[u8]>,
    {
        match self {
            Self::Ethereum(tx) => tx.to_compact(buf),
            Self::Berachain => {
                buf.put_u8(POL_TX_TYPE);
                COMPACT_EXTENDED_IDENTIFIER_FLAG
            }
        }
    }

    // For backwards compatibility purposes only 2 bits of the type are encoded in the identifier
    // parameter. In the case of a [`COMPACT_EXTENDED_IDENTIFIER_FLAG`], the full transaction type
    // is read from the buffer as a single byte.
    fn from_compact(mut buf: &[u8], identifier: usize) -> (Self, &[u8]) {
        use reth_codecs::txtype::*;

        let tx_type = match identifier {
            COMPACT_IDENTIFIER_LEGACY => Self::Ethereum(TxType::Legacy),
            COMPACT_IDENTIFIER_EIP2930 => Self::Ethereum(TxType::Eip2930),
            COMPACT_IDENTIFIER_EIP1559 => Self::Ethereum(TxType::Eip1559),
            COMPACT_EXTENDED_IDENTIFIER_FLAG => {
                let tx_type_byte = buf.get_u8();
                match tx_type_byte {
                    POL_TX_TYPE => Self::Berachain,
                    _ => panic!("Unsupported BerachainTxType extended identifier: {tx_type_byte}"),
                }
            }
            _ => panic!("Unknown identifier for BerachainTxType: {identifier}"),
        };
        (tx_type, buf)
    }
}

impl InMemorySize for BerachainTxType {
    fn size(&self) -> usize {
        size_of::<Self>()
    }
}

impl Compress for BerachainTxType {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        reth_codecs::Compact::to_compact(self, buf);
    }
}

impl Decompress for BerachainTxType {
    fn decompress(value: &[u8]) -> Result<Self, DatabaseError> {
        let (tx, _) = reth_codecs::Compact::from_compact(value, value.len());
        Ok(tx)
    }
}
