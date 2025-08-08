use crate::chainspec::BerachainChainSpec;
use alloy_genesis::Genesis;
use std::sync::Arc;

/// Load the Bepolia testnet chainspec from the genesis file
pub fn bepolia_chainspec() -> Arc<BerachainChainSpec> {
    let genesis_json = include_str!("bepolia-genesis.json");
    let genesis: Genesis =
        serde_json::from_str(genesis_json).expect("Failed to parse bepolia genesis file");
    Arc::new(BerachainChainSpec::from(genesis))
}
