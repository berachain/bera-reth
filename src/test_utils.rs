use crate::chainspec::BerachainChainSpec;
use reth_cli::chainspec::parse_genesis;
use std::sync::Arc;

/// Use the Bepolia Testnet chainspec for testing
pub fn bepolia_chainspec() -> Arc<BerachainChainSpec> {
    let genesis_json = include_str!("../tests/fixtures/bepolia-genesis.json");
    let genesis = parse_genesis(genesis_json).expect("Failed to parse bepolia genesis");
    Arc::new(BerachainChainSpec::from(genesis))
}
