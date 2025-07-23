//! Compact implementation for [`BerachainTxType`]

use crate::transaction::{BerachainTxType, POL_TX_TYPE};
use alloy_consensus::TxType;
use alloy_eips::eip2718::{EIP4844_TX_TYPE_ID, EIP7702_TX_TYPE_ID};
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

    fn from_compact(mut buf: &[u8], identifier: usize) -> (Self, &[u8]) {
        use reth_codecs::txtype::*;

        let tx_type = match identifier {
            COMPACT_IDENTIFIER_LEGACY => Self::Ethereum(TxType::Legacy),
            COMPACT_IDENTIFIER_EIP2930 => Self::Ethereum(TxType::Eip2930),
            COMPACT_IDENTIFIER_EIP1559 => Self::Ethereum(TxType::Eip1559),
            COMPACT_EXTENDED_IDENTIFIER_FLAG => {
                let extended_identifier = buf.get_u8();
                match extended_identifier {
                    POL_TX_TYPE => Self::Berachain,
                    EIP4844_TX_TYPE_ID => Self::Ethereum(TxType::Eip4844),
                    EIP7702_TX_TYPE_ID => Self::Ethereum(TxType::Eip7702),
                    _ => panic!(
                        "Unsupported BerachainTxType extended identifier: {extended_identifier}"
                    ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::TxType;

    #[test]
    fn test_eip4844_compact_roundtrip() {
        let tx_type = BerachainTxType::Ethereum(TxType::Eip4844);

        let mut buf = Vec::new();
        let identifier = tx_type.to_compact(&mut buf);

        let (decoded, _) = BerachainTxType::from_compact(&buf, identifier);
        assert_eq!(tx_type, decoded);
    }

    #[test]
    fn test_eip7702_compact_roundtrip() {
        let tx_type = BerachainTxType::Ethereum(TxType::Eip7702);

        let mut buf = Vec::new();
        let identifier = tx_type.to_compact(&mut buf);

        let (decoded, _) = BerachainTxType::from_compact(&buf, identifier);
        assert_eq!(tx_type, decoded);
    }

    #[test]
    fn test_berachain_pol_compact_roundtrip() {
        let tx_type = BerachainTxType::Berachain;

        let mut buf = Vec::new();
        let identifier = tx_type.to_compact(&mut buf);

        let (decoded, _) = BerachainTxType::from_compact(&buf, identifier);
        assert_eq!(tx_type, decoded);
    }

    /// Test backwards compatibility: Ethereum TxType -> compact -> BerachainTxType
    /// This ensures existing Ethereum transaction data can be read by Berachain
    #[test]
    fn test_backwards_compatibility_ethereum_to_berachain() {
        let ethereum_types = vec![
            TxType::Legacy,
            TxType::Eip2930,
            TxType::Eip1559,
            TxType::Eip4844,
            TxType::Eip7702,
        ];

        for eth_type in ethereum_types {
            // Compact using standard Ethereum TxType
            let mut buf = Vec::new();
            let identifier = eth_type.to_compact(&mut buf);

            // Decompress using BerachainTxType
            let (berachain_type, _) = BerachainTxType::from_compact(&buf, identifier);

            // Should convert to BerachainTxType::Ethereum variant
            match berachain_type {
                BerachainTxType::Ethereum(decoded_eth_type) => {
                    assert_eq!(
                        decoded_eth_type, eth_type,
                        "Ethereum type {eth_type:?} should round-trip correctly"
                    );
                }
                BerachainTxType::Berachain => {
                    panic!(
                        "Ethereum type {eth_type:?} should not decode as Berachain POL transaction"
                    );
                }
            }
        }
    }

    /// Test that BerachainTxType can decompress data originally compressed by Ethereum TxType
    /// for specific transaction types that use extended identifiers
    #[test]
    fn test_backwards_compatibility_extended_identifiers() {
        // Test EIP-4844 (blob transactions)
        let eip4844_type = TxType::Eip4844;
        let mut buf = Vec::new();
        let identifier = eip4844_type.to_compact(&mut buf);

        let (decoded, _) = BerachainTxType::from_compact(&buf, identifier);
        assert_eq!(decoded, BerachainTxType::Ethereum(TxType::Eip4844));

        // Test EIP-7702 (set code transactions)
        let eip7702_type = TxType::Eip7702;
        let mut buf = Vec::new();
        let identifier = eip7702_type.to_compact(&mut buf);

        let (decoded, _) = BerachainTxType::from_compact(&buf, identifier);
        assert_eq!(decoded, BerachainTxType::Ethereum(TxType::Eip7702));
    }

    /// Test that standard Ethereum types (0-2) use direct identifiers
    /// and don't interfere with Berachain's extended identifier usage
    #[test]
    fn test_backwards_compatibility_direct_identifiers() {
        // These should use direct identifiers, not extended
        let direct_types = vec![
            (TxType::Legacy, BerachainTxType::Ethereum(TxType::Legacy)),
            (TxType::Eip2930, BerachainTxType::Ethereum(TxType::Eip2930)),
            (TxType::Eip1559, BerachainTxType::Ethereum(TxType::Eip1559)),
        ];

        for (eth_type, expected_berachain_type) in direct_types {
            let mut buf = Vec::new();
            let identifier = eth_type.to_compact(&mut buf);

            // Direct identifiers should not write to buffer
            assert!(buf.is_empty(), "Direct identifier types should not write to buffer");

            let (decoded, _) = BerachainTxType::from_compact(&buf, identifier);
            assert_eq!(decoded, expected_berachain_type);
        }
    }
}
