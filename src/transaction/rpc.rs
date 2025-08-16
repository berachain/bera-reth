use super::PoLTx;
use alloy_consensus::Transaction;
use alloy_primitives::{Address, Bytes, ChainId, U256};
use serde::{Deserialize, Serialize};

/// RPC wrapper for PoL transactions with all required Ethereum JSON-RPC fields
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoLTxRpc {
    // Core transaction fields
    #[serde(with = "alloy_serde::quantity")]
    pub chain_id: ChainId,
    // Skip 'from' field as it's derived from recover_signer in RPC
    #[serde(skip)]
    pub from: Address,
    pub to: Address,
    #[serde(with = "alloy_serde::quantity")]
    pub nonce: u64,
    #[serde(with = "alloy_serde::quantity", rename = "gas")]
    pub gas_limit: u64,
    #[serde(with = "alloy_serde::quantity")]
    pub gas_price: u128,
    pub input: Bytes,

    // Additional RPC-required fields for Ethereum JSON-RPC compatibility
    pub value: U256, // Always 0 for PoL transactions
    #[serde(with = "alloy_serde::quantity")]
    pub max_fee_per_gas: u128, // Same as gas_price for PoL
    #[serde(with = "alloy_serde::quantity")]
    pub max_priority_fee_per_gas: u128, // Same as gas_price for PoL
    #[serde(with = "alloy_serde::quantity")]
    pub v: u64, // 0 - no real signature for PoL
    pub r: U256,     // 0 - no real signature for PoL
    pub s: U256,     // 0 - no real signature for PoL
}

impl From<&PoLTx> for PoLTxRpc {
    fn from(pol_tx: &PoLTx) -> Self {
        Self {
            chain_id: pol_tx.chain_id().unwrap_or_default(),
            from: pol_tx.from,
            to: pol_tx.to().unwrap_or_default(),
            nonce: pol_tx.nonce(),
            gas_limit: pol_tx.gas_limit(),
            gas_price: pol_tx.gas_price().unwrap_or_default(),
            input: pol_tx.input().clone(),
            // RPC-required fields with appropriate values
            value: pol_tx.value(),
            max_fee_per_gas: pol_tx.max_fee_per_gas(),
            max_priority_fee_per_gas: pol_tx.max_priority_fee_per_gas().unwrap_or_default(),
            v: 0,          // No real signature for PoL
            r: U256::ZERO, // No real signature for PoL
            s: U256::ZERO, // No real signature for PoL
        }
    }
}

impl From<PoLTx> for PoLTxRpc {
    fn from(pol_tx: PoLTx) -> Self {
        Self::from(&pol_tx)
    }
}

impl From<PoLTxRpc> for PoLTx {
    fn from(rpc: PoLTxRpc) -> Self {
        Self {
            chain_id: rpc.chain_id,
            from: rpc.from,
            to: rpc.to,
            nonce: rpc.nonce,
            gas_limit: rpc.gas_limit,
            gas_price: rpc.gas_price,
            input: rpc.input,
            // Note: RPC fields (value, max_fee_per_gas, etc.) are not stored in the core PoLTx
            // struct They are only used for serialization via PoLTxRpc wrapper
        }
    }
}

impl serde::Serialize for PoLTx {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PoLTxRpc::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PoLTx {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialize to PoLTxRpc first, then convert to PoLTx
        let rpc = PoLTxRpc::deserialize(deserializer)?;
        Ok(rpc.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::POL_TX_MAX_PRIORITY_FEE_PER_GAS;

    fn create_test_pol_tx() -> PoLTx {
        PoLTx {
            chain_id: alloy_primitives::ChainId::from(80084u64),
            from: alloy_primitives::Address::ZERO,
            to: alloy_primitives::Address::from([1u8; 20]),
            nonce: 42,
            gas_limit: 21000,
            gas_price: 1000000000u128,
            input: Bytes::from("test data"),
        }
    }

    #[test]
    fn test_pol_tx_serialization() {
        let pol_tx = create_test_pol_tx();

        // Test JSON serialization with RPC fields
        let json = serde_json::to_string_pretty(&pol_tx).expect("Should serialize to JSON");
        println!("PoL transaction JSON:\n{json}");

        // Verify key fields are present in camelCase
        assert!(json.contains("\"chainId\""));
        assert!(json.contains("\"nonce\""));
        assert!(json.contains("\"gas\""));
        assert!(json.contains("\"gasPrice\""));
        assert!(json.contains("\"value\""));
        assert!(json.contains("\"maxFeePerGas\""));
        assert!(json.contains("\"maxPriorityFeePerGas\""));
        assert!(json.contains("\"v\""));
        assert!(json.contains("\"r\""));
        assert!(json.contains("\"s\""));

        // Test that value is "0x0" (zero)
        assert!(json.contains("\"value\": \"0x0\""));

        // Access list should not be present in JSON (skipped)
        assert!(!json.contains("\"accessList\""));

        // Test deserialization (should work with derived Deserialize)
        let deserialized: PoLTx =
            serde_json::from_str(&json).expect("Should deserialize from JSON");
        assert_eq!(deserialized.chain_id, pol_tx.chain_id);
        assert_eq!(deserialized.nonce, pol_tx.nonce);
        assert_eq!(deserialized.gas_limit, pol_tx.gas_limit);
        assert_eq!(deserialized.gas_price, pol_tx.gas_price);
    }

    #[test]
    fn test_pol_tx_rpc_conversions() {
        let pol_tx = create_test_pol_tx();

        // Test PoLTx -> PoLTxRpc conversion
        let rpc: PoLTxRpc = pol_tx.clone().into();
        assert_eq!(rpc.chain_id, pol_tx.chain_id);
        assert_eq!(rpc.nonce, pol_tx.nonce);
        assert_eq!(rpc.gas_limit, pol_tx.gas_limit);
        assert_eq!(rpc.gas_price, pol_tx.gas_price);
        assert_eq!(rpc.value, U256::ZERO);
        assert_eq!(rpc.max_fee_per_gas, pol_tx.gas_price);
        assert_eq!(rpc.max_priority_fee_per_gas, POL_TX_MAX_PRIORITY_FEE_PER_GAS);
        assert_eq!(rpc.v, 0);
        assert_eq!(rpc.r, U256::ZERO);
        assert_eq!(rpc.s, U256::ZERO);

        // Test PoLTxRpc -> PoLTx conversion
        let converted_back: PoLTx = rpc.into();
        assert_eq!(converted_back.chain_id, pol_tx.chain_id);
        assert_eq!(converted_back.nonce, pol_tx.nonce);
        assert_eq!(converted_back.gas_limit, pol_tx.gas_limit);
        assert_eq!(converted_back.gas_price, pol_tx.gas_price);
        assert_eq!(converted_back.input, pol_tx.input);
    }
}
