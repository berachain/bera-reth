use super::pol::create_pol_transaction;
use crate::{
    chainspec::BerachainChainSpec, primitives::header::BlsPublicKey,
    transaction::BerachainTxEnvelope,
};
use alloy_eips::Encodable2718;
use alloy_primitives::{Address, U256, hex, keccak256};
use reth_chainspec::EthChainSpec;
use std::sync::Arc;

/// Test that POL transactions create consistent hashes with known inputs
/// This helps debug hash mismatches with other implementations (e.g., bera-geth)
#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    fn create_test_chain_spec() -> Arc<BerachainChainSpec> {
        // Create a default chain spec
        let spec = BerachainChainSpec {
            pol_contract_address: address!("0x4200000000000000000000000000000000000042"),
            ..Default::default()
        };
        Arc::new(spec)
    }

    #[test]
    fn test_pol_transaction_hash_consistency() {
        let chain_spec = create_test_chain_spec();

        // Use exact same test values as bera-geth test
        let block_number = U256::from(10);
        let base_fee = 1000000000u64; // 1 gwei

        // Create fixed 48-byte BLS pubkey for reproducible tests (same as bera-geth)
        let pubkey_hex = "746573745f7075626b65795f666f725f636f6e73697374656e745f686173685f76616c69646174696f6e5f5f00000000";
        let pubkey_bytes = hex::decode(pubkey_hex).expect("Invalid hex");
        let mut pubkey_array = [0u8; 48];
        pubkey_array.copy_from_slice(&pubkey_bytes);
        let prev_proposer_pubkey = BlsPublicKey::from(pubkey_array);

        println!("Test Parameters:");
        println!("  Chain ID: {}", chain_spec.chain().id());
        println!("  Block Number: {}", block_number);
        println!("  Nonce (block - 1): {}", block_number - U256::from(1));
        println!("  Base Fee: {}", base_fee);
        println!("  Pubkey: {}", pubkey_hex);

        // Create POL transaction
        let pol_envelope = create_pol_transaction(
            chain_spec.clone(),
            prev_proposer_pubkey,
            block_number,
            base_fee,
        )
        .expect("Failed to create POL transaction");

        // Extract the PoL transaction from the envelope
        let pol_tx = match pol_envelope {
            BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx.into_inner(),
            _ => panic!("Expected Berachain POL transaction"),
        };

        // Get transaction hash
        let tx_hash = pol_tx.tx_hash();
        println!("Generated TX Hash: {:#x}", tx_hash);

        println!("POL Transaction Details:");
        println!("  ChainID: {}", pol_tx.chain_id);
        println!("  From: {:#x}", pol_tx.from);
        println!("  To: {:#x}", pol_tx.to);
        println!("  Nonce: {}", pol_tx.nonce);
        println!("  GasLimit: {}", pol_tx.gas_limit);
        println!("  GasPrice: {}", pol_tx.gas_price);
        println!("  Data length: {} bytes", pol_tx.input.len());
        println!("  Data (hex): {}", hex::encode(&pol_tx.input));

        // Log RLP encoding details for debugging
        let mut encoded = Vec::new();
        pol_tx.encode_2718(&mut encoded);
        println!("RLP encoded length: {} bytes", encoded.len());
        println!("RLP encoded (hex): {}", hex::encode(&encoded));

        // Test hash consistency - run same test multiple times
        for i in 0..5 {
            let pol_envelope2 = create_pol_transaction(
                chain_spec.clone(),
                prev_proposer_pubkey,
                block_number,
                base_fee,
            )
            .expect(&format!("Failed to create POL transaction on iteration {}", i));

            let pol_tx2 = match pol_envelope2 {
                BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx.into_inner(),
                _ => panic!("Expected Berachain POL transaction"),
            };

            let tx_hash2 = pol_tx2.tx_hash();
            assert_eq!(tx_hash, tx_hash2, "Hash should be consistent across multiple creations");
        }

        // Expected hash from bera-geth test
        let expected_hash_from_geth =
            "0xb25ddbd12759e62bb51e3feb91afb88f3528578c72f469ca0793bfd4d4c1ff08";
        println!("Expected hash from bera-geth: {}", expected_hash_from_geth);

        // Compare hashes - this will likely fail initially, showing us the difference
        if format!("{:#x}", tx_hash) != expected_hash_from_geth {
            println!("❌ HASH MISMATCH DETECTED:");
            println!("  bera-reth hash: {:#x}", tx_hash);
            println!("  bera-geth hash: {}", expected_hash_from_geth);
            println!("  This confirms the RLP encoding difference we suspected!");
        } else {
            println!("✅ Hashes match between implementations!");
        }
    }

    #[test]
    fn test_pol_transaction_rlp_structure() {
        let chain_spec = create_test_chain_spec();

        // Use minimal values to make RLP debugging easier
        let block_number = U256::from(2); // nonce will be 1
        let base_fee = 1u64;

        // Create minimal pubkey
        let mut pubkey_array = [0u8; 48];
        pubkey_array[47] = 1; // Just set last byte to 1
        let prev_proposer_pubkey = BlsPublicKey::from(pubkey_array);

        let pol_envelope =
            create_pol_transaction(chain_spec, prev_proposer_pubkey, block_number, base_fee)
                .expect("Failed to create POL transaction");

        let pol_tx = match pol_envelope {
            BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx.into_inner(),
            _ => panic!("Expected Berachain POL transaction"),
        };

        println!("Minimal POL Transaction for RLP Analysis:");
        println!(
            "  ChainID: {} (bytes: {})",
            pol_tx.chain_id,
            hex::encode(pol_tx.chain_id.to_be_bytes())
        );
        println!("  From: {:#x}", pol_tx.from);
        println!("  To: {:#x}", pol_tx.to);
        println!("  Nonce: {}", pol_tx.nonce);
        println!("  GasLimit: {}", pol_tx.gas_limit);
        println!(
            "  GasPrice: {} (bytes: {})",
            pol_tx.gas_price,
            hex::encode(pol_tx.gas_price.to_be_bytes())
        );
        println!("  Data: {} bytes", pol_tx.input.len());

        // Get RLP encoding
        let mut encoded = Vec::new();
        pol_tx.encode_2718(&mut encoded);

        println!("Full RLP (hex): {}", hex::encode(&encoded));
        println!("Transaction type byte: 0x{:02x}", encoded[0]);
        println!("RLP payload: {}", hex::encode(&encoded[1..]));

        let tx_hash = pol_tx.tx_hash();
        println!("Minimal TX Hash: {:#x}", tx_hash);
    }

    #[test]
    fn test_gas_price_encoding_specifically() {
        // Test specifically how u128 vs big.Int encode differently
        let test_values = vec![
            1u128,
            1000000000u128, // 1 gwei
            2000000000u128, // 2 gwei
            u128::MAX,
        ];

        for (i, gas_price) in test_values.iter().enumerate() {
            println!("Gas Price Test {}: {}", i, gas_price);

            // Test u128 RLP encoding (what bera-reth uses)
            let mut u128_encoded = Vec::new();
            use alloy_rlp::Encodable;
            gas_price.encode(&mut u128_encoded);
            println!("  u128 RLP encoded: {}", hex::encode(&u128_encoded));

            // Also show big-endian bytes representation
            let be_bytes = gas_price.to_be_bytes();
            println!("  u128 big-endian bytes: {}", hex::encode(&be_bytes));
        }
    }

    #[test]
    fn test_manual_rlp_construction() {
        // Manually construct the exact same RLP as Go to understand the difference
        use alloy_rlp::{Encodable, Header};
        // use alloy_rlp::BufMut;

        // Use same parameters as the consistency test
        let chain_id = 80087u64;
        let from = Address::from(hex!("ffffFFFfFFffffffffffffffFfFFFfffFFFfFFfE"));
        let to = Address::from(hex!("4200000000000000000000000000000000000042"));
        let nonce = 9u64;
        let gas_limit = 30000000u64;
        let gas_price = 1000000000u128;

        // Construct the calldata (should match Go version)
        let pubkey_hex = "746573745f7075626b65795f666f725f636f6e73697374656e745f686173685f76616c69646174696f6e5f5f00000000";
        let pubkey_bytes = hex::decode(pubkey_hex).expect("Invalid hex");

        // This should create the same distributeFor calldata as Go
        let mut calldata = Vec::new();
        // Function selector for distributeFor(bytes)
        calldata.extend_from_slice(&hex!("60644a6b"));
        // Offset to data (32 bytes)
        calldata.extend_from_slice(&[0u8; 31]);
        calldata.push(0x20);
        // Length of data (48 bytes = 0x30)
        calldata.extend_from_slice(&[0u8; 31]);
        calldata.push(0x30);
        // The actual pubkey data
        calldata.extend_from_slice(&pubkey_bytes);

        println!("Manual calldata construction: {}", hex::encode(&calldata));

        // Now manually construct RLP list
        let mut manual_rlp = Vec::new();

        // Calculate payload length first
        let payload_length = chain_id.length() +
            from.length() +
            to.length() +
            nonce.length() +
            gas_limit.length() +
            gas_price.length() +
            calldata.length();

        println!("Calculated payload length: {}", payload_length);

        // Encode list header
        Header { list: true, payload_length }.encode(&mut manual_rlp);

        // Encode each field
        chain_id.encode(&mut manual_rlp);
        from.encode(&mut manual_rlp);
        to.encode(&mut manual_rlp);
        nonce.encode(&mut manual_rlp);
        gas_limit.encode(&mut manual_rlp);
        gas_price.encode(&mut manual_rlp);
        calldata.encode(&mut manual_rlp);

        println!("Manual RLP payload: {}", hex::encode(&manual_rlp));

        // Add transaction type and hash
        let mut with_type = Vec::new();
        with_type.push(0x7e); // transaction type
        with_type.extend_from_slice(&manual_rlp);

        println!("Manual full encoding: {}", hex::encode(&with_type));

        let manual_hash = keccak256(&with_type);
        println!("Manual hash: {:#x}", manual_hash);
    }
}
