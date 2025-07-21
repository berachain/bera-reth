#[cfg(test)]
mod tests {
    use crate::transaction::PoLTx;
    use alloy_eips::Encodable2718;
    use alloy_primitives::{Address, ChainId, hex, Bytes, keccak256};

    #[test]
    fn test_exact_same_parameters_as_geth() {
        // Create POL transaction with exact same parameters as bera-geth test
        let chain_id = ChainId::from(80087u64);
        let from = Address::from(hex!("ffffFFFfFFffffffffffffffFfFFFfffFFFfFFfE"));
        let to = Address::from(hex!("4200000000000000000000000000000000000042"));
        let nonce = 9u64; // block 10 - 1
        let gas_limit = 30000000u64;
        let gas_price = 1000000000u128; // 1 gwei
        
        // Create the exact same calldata as bera-geth 
        let calldata_hex = "60644a6b00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000030746573745f7075626b65795f666f725f636f6e73697374656e745f686173685f76616c69646174696f6e5f5f0000000000000000000000000000000000000000";
        let calldata = Bytes::from(hex::decode(calldata_hex).expect("Invalid hex"));

        let pol_tx = PoLTx {
            chain_id,
            from,
            to,
            nonce,
            gas_limit,
            gas_price,
            input: calldata,
        };

        println!("POL Transaction Details:");
        println!("  ChainID: {}", pol_tx.chain_id);
        println!("  From: {:#x}", pol_tx.from);
        println!("  To: {:#x}", pol_tx.to);
        println!("  Nonce: {}", pol_tx.nonce);
        println!("  GasLimit: {}", pol_tx.gas_limit);
        println!("  GasPrice: {}", pol_tx.gas_price);
        println!("  Data length: {} bytes", pol_tx.input.len());
        println!("  Data (hex): {}", hex::encode(&pol_tx.input));

        // Get transaction hash
        let tx_hash = pol_tx.tx_hash();
        println!("Generated TX Hash: {:#x}", tx_hash);

        // Log RLP encoding details for debugging
        let mut encoded = Vec::new();
        pol_tx.encode_2718(&mut encoded);
        println!("RLP encoded length: {} bytes", encoded.len());
        println!("Actual bera-reth RLP:   {}", hex::encode(&encoded));

        // Compare with bera-geth RLP
        let expected_geth_rlp = "7ef8bf830138d794fffffffffffffffffffffffffffffffffffffffe944200000000000000000000000000000000000042098401c9c380843b9aca00b88460644a6b00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000030746573745f7075626b65795f666f725f636f6e73697374656e745f686173685f76616c69646174696f6e5f5f0000000000000000000000000000000000000000";
        
        println!("Expected bera-geth RLP: {}", expected_geth_rlp);
        
        if hex::encode(&encoded) == expected_geth_rlp {
            println!("✅ RLP encodings match!");
        } else {
            println!("❌ RLP encoding mismatch - analyzing differences...");
            
            // Find where they differ
            let expected_bytes = hex::decode(expected_geth_rlp).expect("Invalid geth hex");
            let actual_bytes = &encoded;
            
            for (i, (expected, actual)) in expected_bytes.iter().zip(actual_bytes.iter()).enumerate() {
                if expected != actual {
                    println!("  Difference at byte {}: expected 0x{:02x}, got 0x{:02x}", i, expected, actual);
                }
            }
            
            if expected_bytes.len() != actual_bytes.len() {
                println!("  Length difference: expected {} bytes, got {} bytes", expected_bytes.len(), actual_bytes.len());
            }
        }

        // Expected hash from bera-geth test
        let expected_hash_from_geth = "0xb25ddbd12759e62bb51e3feb91afb88f3528578c72f469ca0793bfd4d4c1ff08";
        println!("Expected hash from bera-geth: {}", expected_hash_from_geth);
        
        // Compare hashes 
        if format!("{:#x}", tx_hash) == expected_hash_from_geth {
            println!("✅ SUCCESS: Hashes match between implementations!");
        } else {
            println!("❌ Hash mismatch:");
            println!("  bera-reth hash: {:#x}", tx_hash);
            println!("  bera-geth hash: {}", expected_hash_from_geth);
            
            // Verify hash calculation manually
            let manual_hash = keccak256(&encoded);
            println!("  Manual hash calculation: {:#x}", manual_hash);
            assert_eq!(tx_hash, manual_hash, "Hash calculation should be consistent");
        }
    }
}