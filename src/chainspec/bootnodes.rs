//! Default P2P bootnodes for Berachain mainnet (chain id 80094).

use reth_network_peers::{NodeRecord, TrustedPeer};
use std::{str::FromStr, sync::LazyLock};

const BERACHAIN_MAINNET_BOOTNODES_RAW: &str = "enode://ce9c87cfe089f6811d26c96913fa3ec10b938d9017fc6246684c74a33679ee34ceca9447180fb509e37bf2b706c2877a82085d34bfd83b5b520ee1288b0fc32f@35.198.109.49:30303,enode://6a35d56cb29734fff7d687908147b24c34dbcbbe97f7415222846a3d11ed4a5ad75dd714700e708b46e44e8fe89ec1b31f111ca49a4a5b9dd2fda1d4f46b158a@34.141.15.100:30303,enode://2c62a49e010cd1b0c1055c16b0a5a15e0f6794ae5027678114ae10905ecd91aa705a526508e0bf73d22002983e0d844df5a6f3dd33ede1e54c46c81707d5f057@34.107.105.176:30303,enode://da94328302a1d1422209d1916744e90b6095a48b2340dcec39b22002c098bb4d58a880dab98eb26edf03fa4705d1b62f99a8c5c14e6666e4726b6d3066d8a4d7@34.95.61.106:30303,enode://19c7671a4844699b481e81a5bcfe7bafc7fefa953c16ebbe1951b1046371e73839e9058de6b7d3c934318fe7e7233dde3621c1c1018eb8b294ea3d4516147150@35.203.82.137:30303,enode://9e10ca450fbc6a15707f054a59e1fd44ab56c5c4f85cd0ecb37b7eadcd512e538cdcaeff0b8fa546e056c79e7427f13b3b60639b5122b4f0488592b5ffb3ad62@35.203.61.36:30303,enode://5339627b5e58ee156ec675fb03d7242659b1a05fa44b73d9f577ad505fc52e0aa887e527b17fb7c7192682001f148bced570f98fb3132683beeccf0404da651e@34.95.40.210:30303";

fn parse_berachain_mainnet_bootnodes() -> Vec<NodeRecord> {
    BERACHAIN_MAINNET_BOOTNODES_RAW
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let peer =
                TrustedPeer::from_str(s).expect("valid Berachain mainnet bootnode enode URL");
            peer.resolve_blocking().expect("Berachain mainnet bootnode resolves to NodeRecord")
        })
        .collect()
}

/// Default discovery bootstrap nodes for Berachain mainnet.
pub static BERACHAIN_MAINNET_BOOTNODES: LazyLock<Vec<NodeRecord>> =
    LazyLock::new(parse_berachain_mainnet_bootnodes);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn berachain_mainnet_bootnode_count() {
        assert_eq!(BERACHAIN_MAINNET_BOOTNODES.len(), 7);
    }
}
