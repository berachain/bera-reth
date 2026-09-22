//! Default P2P bootnodes for Berachain mainnet (80094) and Bepolia (80069).

use reth_network_peers::{NodeRecord, TrustedPeer};
use std::{str::FromStr, sync::LazyLock};

const BERACHAIN_MAINNET_BOOTNODES_RAW: &str = "enode://0c5a4a3c0e81fce2974e4d317d88df783731183d534325e32e0fdf8f4b119d7889fa254d3a38890606ec300d744e2aa9c87099a4a032f5c94efe53f3fcdfecfe@34.64.176.79:30303,enode://b6a3137d3a36ef37c4d31843775a9dc293f41bcbde33b6309c80b1771b6634827cd188285136a57474427bd8845adc2f6fe2e0b106bd58d14795b08910b9c326@34.64.181.70:30303,enode://0b6633300614bc2b9749aee0cace7a091ec5348762aee7b1d195f7616d03a9409019d9bef336624bab72e0d069cd4cf0b0de6fbbf53f04f6b6e4c5b39c6bdca6@34.64.39.31:30303,enode://552b001abebb5805fcd734ad367cd05d9078d18f23ec598d7165460fadcfc51116ad95c418f7ea9a141aa8cbc496c8bea3322b67a5de0d3380f11aab1a797513@34.64.183.158:30303,enode://5b037f66099d5ded86eb7e1619f6d06ceb15609e8cc345ced22a4772b06178004e1490a3cd32fd1222789de4c6e4021c2d648a3d750f6d5323e64b771bbd8de7@34.87.142.180:30303,enode://846db253c53753d3ea1197aec296306dc84c25f3afdf142b65cb0fe0f984de55072daa3bbf05a9aea046a38a2292403137b6eafefd5646fcf62120b74e3b898d@34.142.170.110:30303,enode://64b7f6ee9bcd942ad4949c70f2077627f078a057dfd930e6e904e12643d8952f5ae87c91e24559765393f244a72c9d5c011d7d5176e59191d38f315db85a20f5@34.126.161.16:30303,enode://cf4d19bfb8ec507427ec882bac0bac85a0c8c9ddaa0ec91b773bb614e5e09d107cd9fbe323b96f62f31c493f8f42cc5495c18b87c08560c5dea1dfd25256dcf6@35.247.162.2:30303,enode://ce9c87cfe089f6811d26c96913fa3ec10b938d9017fc6246684c74a33679ee34ceca9447180fb509e37bf2b706c2877a82085d34bfd83b5b520ee1288b0fc32f@34.40.28.159:30303,enode://6a35d56cb29734fff7d687908147b24c34dbcbbe97f7415222846a3d11ed4a5ad75dd714700e708b46e44e8fe89ec1b31f111ca49a4a5b9dd2fda1d4f46b158a@35.234.82.236:30303,enode://2c62a49e010cd1b0c1055c16b0a5a15e0f6794ae5027678114ae10905ecd91aa705a526508e0bf73d22002983e0d844df5a6f3dd33ede1e54c46c81707d5f057@34.159.19.225:30303,enode://da94328302a1d1422209d1916744e90b6095a48b2340dcec39b22002c098bb4d58a880dab98eb26edf03fa4705d1b62f99a8c5c14e6666e4726b6d3066d8a4d7@34.95.30.190:30303,enode://19c7671a4844699b481e81a5bcfe7bafc7fefa953c16ebbe1951b1046371e73839e9058de6b7d3c934318fe7e7233dde3621c1c1018eb8b294ea3d4516147150@34.47.60.196:30303,enode://9e10ca450fbc6a15707f054a59e1fd44ab56c5c4f85cd0ecb37b7eadcd512e538cdcaeff0b8fa546e056c79e7427f13b3b60639b5122b4f0488592b5ffb3ad62@34.95.42.80:30303,enode://5339627b5e58ee156ec675fb03d7242659b1a05fa44b73d9f577ad505fc52e0aa887e527b17fb7c7192682001f148bced570f98fb3132683beeccf0404da651e@35.203.11.5:30303";

const BERACHAIN_BEPOLIA_BOOTNODES_RAW: &str = "enode://5c0d582c19ea9f19928cfd6b7e156372d051b5720f67a444c38b671b8119bb097abfd1e4b868a389ab4a65bb9b405ef837df0ae195c0f32a33c61c39ca54e8ab@34.107.71.151:30303,enode://59aec227e87f4cd7c0a24c6cb0f870ef77abcc0c6d640f5a536c597a206b3505f7963f6459c265952b7bed3c3f260edf8a1412bbdb05f0e59d6bd612dc4bf077@34.141.48.88:30303,enode://47f41b9ab5a45e880a78330d2ae3f95a61f5cb41f203bbc9c9ff0e37778fc6c7fd46a6ee103e65ac36df8c024075a33f535a03cd8d800800c27fa2699fa0182b@34.47.28.251:30303,enode://fe6d2429b582de7daf387c6e5436f05d9185965267b72b8b6b4924125b50afdad765a820213d73d2afbfe64a721160a1a94750b4874ed97ccbe97c51443d1c42@34.95.21.165:30303,enode://869293789a1bbdcbdd0d8c2f1eed560d20e4b4b23b21abf575c87158aea59f0396781b54fcedd8db01a6db075de71dbd29e75cfdbb83cea9cea1a6bd31bec74c@67.213.122.179:30303,enode://e4b804a4fc7833f03b6033542d72d026727a651e8b4369b42b8e6f22a1ebbe649c22b646e7180cd05843c4992c477d1999cbec101d97f0f7edc950495b846eb5@206.223.224.23:30303";

fn parse_bootnodes(raw: &str, network_label: &'static str) -> Vec<NodeRecord> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let peer = TrustedPeer::from_str(s)
                .unwrap_or_else(|_| panic!("valid {network_label} bootnode enode URL: {s}"));
            peer.resolve_blocking()
                .unwrap_or_else(|_| panic!("{network_label} bootnode resolves: {s}"))
        })
        .collect()
}

/// Default discovery bootstrap nodes for Berachain mainnet.
pub static BERACHAIN_MAINNET_BOOTNODES: LazyLock<Vec<NodeRecord>> =
    LazyLock::new(|| parse_bootnodes(BERACHAIN_MAINNET_BOOTNODES_RAW, "Berachain mainnet"));

/// Default discovery bootstrap nodes for Bepolia testnet.
pub static BERACHAIN_BEPOLIA_BOOTNODES: LazyLock<Vec<NodeRecord>> =
    LazyLock::new(|| parse_bootnodes(BERACHAIN_BEPOLIA_BOOTNODES_RAW, "Bepolia"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn berachain_mainnet_bootnode_count() {
        assert_eq!(BERACHAIN_MAINNET_BOOTNODES.len(), 15);
    }

    #[test]
    fn bepolia_bootnode_count() {
        assert_eq!(BERACHAIN_BEPOLIA_BOOTNODES.len(), 6);
    }
}
