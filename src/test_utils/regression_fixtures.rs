use crate::primitives::header::{BerachainHeader, BlsPublicKey};
use alloy_consensus::EMPTY_OMMER_ROOT_HASH;
use alloy_primitives::{Address, B64, Bytes, U256, address, b256, bloom, bytes};

pub fn holesky_berachain_header() -> BerachainHeader {
    BerachainHeader {
        parent_hash: b256!("0x8605e0c46689f66b3deed82598e43d5002b71a929023b665228728f0c6e62a95"),
        ommers_hash: EMPTY_OMMER_ROOT_HASH,
        beneficiary: address!("0xc6e2459991bfe27cca6d86722f35da23a1e4cb97"),
        state_root: b256!("0xedad188ca5647d62f4cca417c11a1afbadebce30d23260767f6f587e9b3b9993"),
        transactions_root: b256!(
            "0x4daf25dc08a841aa22aa0d3cb3e1f159d4dcaf6a6063d4d36bfac11d3fdb63ee"
        ),
        receipts_root: b256!("0x1a1500328e8ade2592bbea1e04f9a9fd8c0142d3175d6e8420984ee159abd0ed"),
        withdrawals_root: Some(b256!(
            "0xd0f7f22d6d915be5a3b9c0fee353f14de5ac5c8ac1850b76ce9be70b69dfe37d"
        )),
        logs_bloom: bloom!(
            "36410880400480e1090a001c408880800019808000125124002100400048442220020000408040423088300004d0000050803000862485a02020011600a5010404143021800881e8e08c402940404002105004820c440051640000809c000011080002300208510808150101000038002500400040000230000000110442800000800204420100008110080200088c1610c0b80000c6008900000340400200200210010111020000200041a2010804801100030a0284a8463820120a0601480244521002a10201100400801101006002001000008000000ce011011041086418609002000128800008180141002003004c00800040940c00c1180ca002890040"
        ),
        difficulty: U256::ZERO,
        number: 0x1db931,
        gas_limit: 0x1c9c380,
        gas_used: 0x440949,
        timestamp: 0x66982980,
        mix_hash: b256!("0x574db0ff0a2243b434ba2a35da8f2f72df08bca44f8733f4908d10dcaebc89f1"),
        nonce: B64::ZERO,
        base_fee_per_gas: Some(0x8),
        blob_gas_used: Some(0x60000),
        excess_blob_gas: Some(0x0),
        parent_beacon_block_root: Some(b256!(
            "0xaa1d9606b7932f2280a19b3498b9ae9eebc6a83f1afde8e45944f79d353db4c1"
        )),
        requests_hash: None,
        prev_proposer_pubkey: None,
        extra_data: bytes!("726574682f76312e302e302f6c696e7578"),
    }
}

pub fn holesky_berachain_header_with_proposer_pubkey() -> BerachainHeader {
    BerachainHeader {
        prev_proposer_pubkey: Some(BlsPublicKey::from([0x42; 48])),
        requests_hash: Some(b256!(
            "0x1111111111111111111111111111111111111111111111111111111111111111"
        )),
        ..holesky_berachain_header()
    }
}

pub fn minimal_pol_tx() -> crate::transaction::PoLTx {
    use alloy_primitives::ChainId;
    crate::transaction::PoLTx {
        chain_id: ChainId::from(80084u64),
        from: Address::ZERO,
        to: Address::from([1u8; 20]),
        nonce: 42,
        gas_limit: 21000,
        gas_price: 1000000000u128,
        input: Bytes::from("test data"),
    }
}

#[cfg(test)]
mod print_regression {
    use super::*;
    use crate::{
        chainspec::BerachainChainSpec,
        test_utils::bepolia_chainspec,
        transaction::{BerachainTxEnvelope, BerachainTxType, pol::create_pol_transaction},
    };
    use alloy_primitives::{Sealable, Sealed, U256};
    use reth_chainspec::EthChainSpec;
    use reth_cli::chainspec::parse_genesis;
    use reth_codecs::Compact;
    use reth_db_api::table::Compress;

    #[test]
    #[ignore = "manual regression values generator"]
    fn print_regression_values() {
        let holesky = holesky_berachain_header();
        let mut buf = Vec::new();
        holesky.compress_to_buf(&mut buf);
        eprintln!("holesky_hash={:#x}", holesky.hash_slow());
        eprintln!("holesky_compact={}", alloy_primitives::hex::encode(&buf));

        let with_pubkey = holesky_berachain_header_with_proposer_pubkey();
        let mut buf = Vec::new();
        with_pubkey.compress_to_buf(&mut buf);
        eprintln!("with_pubkey_hash={:#x}", with_pubkey.hash_slow());
        eprintln!("with_pubkey_compact={}", alloy_primitives::hex::encode(&buf));

        let pol = minimal_pol_tx();
        eprintln!("minimal_pol_hash={:#x}", pol.hash_slow());
        let mut buf = Vec::new();
        pol.to_compact(&mut buf);
        eprintln!("minimal_pol_compact={}", alloy_primitives::hex::encode(&buf));

        let envelope = BerachainTxEnvelope::Berachain(Sealed::new(pol.clone()));
        let mut buf = Vec::new();
        envelope.compress_to_buf(&mut buf);
        eprintln!("minimal_pol_envelope_compact={}", alloy_primitives::hex::encode(&buf));

        let tx_type = BerachainTxType::Berachain;
        let mut buf = Vec::new();
        let id = tx_type.to_compact(&mut buf);
        eprintln!("berachain_tx_type_id={id}");
        eprintln!("berachain_tx_type_compact={}", alloy_primitives::hex::encode(&buf));

        let chain_spec = bepolia_chainspec();
        let pubkey = BlsPublicKey::from([1u8; 48]);
        let pol_prod =
            match create_pol_transaction(chain_spec, pubkey, U256::from(10), 1000).unwrap() {
                BerachainTxEnvelope::Berachain(sealed) => sealed,
                _ => panic!("expected pol"),
            };
        eprintln!("production_pol_hash={:#x}", pol_prod.hash());

        let bepolia_json = include_str!("../../tests/fixtures/bepolia-genesis.json");
        let bepolia = BerachainChainSpec::from(parse_genesis(bepolia_json).unwrap());
        eprintln!("bepolia_genesis_hash={:#x}", bepolia.genesis_hash());

        let mainnet_json = include_str!("../../tests/fixtures/mainnet-genesis.json");
        let mainnet = BerachainChainSpec::from(parse_genesis(mainnet_json).unwrap());
        eprintln!("mainnet_genesis_hash={:#x}", mainnet.genesis_hash());
    }
}
