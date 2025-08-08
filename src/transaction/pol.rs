use crate::{
    chainspec::BerachainChainSpec,
    primitives::header::BlsPublicKey,
    transaction::{BerachainTxEnvelope, PoLTx},
};
use alloy_primitives::{Bytes, Sealed, U256};
use alloy_sol_macro::sol;
use alloy_sol_types::SolCall;
use reth::{consensus::ConsensusError, revm::handler::SYSTEM_ADDRESS};
use reth_chainspec::EthChainSpec;
use reth_evm::block::{BlockExecutionError, InternalBlockExecutionError};
use std::sync::Arc;

pub const POL_TX_GAS_LIMIT: u64 = 30_000_000;

pub fn create_pol_transaction(
    chain_spec: Arc<BerachainChainSpec>,
    prev_proposer_pubkey: BlsPublicKey,
    block_number: U256,
    base_fee: u64,
) -> Result<BerachainTxEnvelope, BlockExecutionError> {
    sol! {
        interface PoLDistributor {
            function distributeFor(bytes calldata pubkey) external;
        }
    }
    let distribute_call =
        PoLDistributor::distributeForCall { pubkey: Bytes::from(prev_proposer_pubkey) };
    let calldata = distribute_call.abi_encode();

    let nonce_u256 = block_number - U256::from(1);
    let nonce = nonce_u256.try_into().map_err(|_| {
        BlockExecutionError::Internal(InternalBlockExecutionError::Other(
            format!(
                "block number overflow for u64 nonce: block_number={block_number}, nonce_u256={nonce_u256}"
            )
            .into(),
        ))
    })?;

    let pol_tx = PoLTx {
        chain_id: chain_spec.chain_id(),
        from: SYSTEM_ADDRESS,
        to: chain_spec.pol_contract(),
        input: Bytes::from(calldata),
        nonce,
        gas_limit: POL_TX_GAS_LIMIT, // this is the env value used in revm for system calls
        gas_price: base_fee.into(),  /* gas price is set to the base fee for RPC
                                      * compatability reasons */
    };

    Ok(BerachainTxEnvelope::Berachain(Sealed::new(pol_tx)))
}

pub fn validate_pol_transaction(
    pol_tx: &Sealed<PoLTx>,
    chain_spec: Arc<BerachainChainSpec>,
    expected_pubkey: BlsPublicKey,
    block_number: U256,
    base_fee: u64,
) -> Result<(), ConsensusError> {
    let expected_tx = create_pol_transaction(chain_spec, expected_pubkey, block_number, base_fee)
        .map_err(|e| {
        ConsensusError::Other(format!("Failed to create expected PoL transaction: {e}"))
    })?;

    let expected_sealed_pol_tx = match expected_tx {
        BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
        _ => return Err(ConsensusError::Other("Expected PoL transaction envelope".into())),
    };

    if pol_tx.hash() != expected_sealed_pol_tx.hash() {
        return Err(ConsensusError::Other(format!(
            "PoL transaction hash mismatch: expected {}, got {}",
            expected_sealed_pol_tx.hash(),
            pol_tx.hash()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;
    use std::sync::Arc;

    fn mock_berachain_chainspec() -> Arc<BerachainChainSpec> {
        crate::test::bepolia_chainspec()
    }

    fn mock_bls_pubkey() -> BlsPublicKey {
        BlsPublicKey::from([1u8; 48])
    }

    #[test]
    fn test_pol_transaction_creation_and_validation() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(10);
        let base_fee = 1000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, block_number, base_fee);

        assert!(pol_tx_envelope.is_ok(), "PoL transaction creation should succeed");

        let pol_tx = match pol_tx_envelope.unwrap() {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec.clone(), pubkey, block_number, base_fee);

        assert!(validation_result.is_ok(), "Valid PoL transaction should pass validation");
    }

    #[test]
    fn test_pol_transaction_validation_wrong_pubkey() {
        let chain_spec = mock_berachain_chainspec();
        let correct_pubkey = mock_bls_pubkey();
        let wrong_pubkey = BlsPublicKey::from([2u8; 48]);
        let block_number = U256::from(10);
        let base_fee = 1000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), correct_pubkey, block_number, base_fee)
                .unwrap();

        let pol_tx = match pol_tx_envelope {
            crate::transaction::BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec, wrong_pubkey, block_number, base_fee);

        assert!(
            validation_result.is_err(),
            "PoL transaction with wrong pubkey should fail validation"
        );
        assert!(validation_result.unwrap_err().to_string().contains("hash mismatch"));
    }

    #[test]
    fn test_pol_transaction_validation_wrong_base_fee() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(10);
        let correct_base_fee = 1000u64;
        let wrong_base_fee = 2000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, block_number, correct_base_fee)
                .unwrap();

        let pol_tx = match pol_tx_envelope {
            BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec, pubkey, block_number, wrong_base_fee);

        assert!(
            validation_result.is_err(),
            "PoL transaction with wrong base fee should fail validation"
        );
        assert!(validation_result.unwrap_err().to_string().contains("hash mismatch"));
    }

    #[test]
    fn test_pol_transaction_validation_wrong_block_number() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let correct_block_number = U256::from(10);
        let wrong_block_number = U256::from(20);
        let base_fee = 1000u64;

        let pol_tx_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, correct_block_number, base_fee)
                .unwrap();

        let pol_tx = match pol_tx_envelope {
            BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let validation_result =
            validate_pol_transaction(&pol_tx, chain_spec, pubkey, wrong_block_number, base_fee);

        assert!(
            validation_result.is_err(),
            "PoL transaction with wrong block number should fail validation"
        );
        assert!(validation_result.unwrap_err().to_string().contains("hash mismatch"));
    }

    #[test]
    fn test_pol_transaction_deterministic_hashes() {
        let chain_spec = mock_berachain_chainspec();
        let pubkey = mock_bls_pubkey();
        let block_number = U256::from(42);
        let base_fee = 1337u64;

        let pol_tx1_envelope =
            create_pol_transaction(chain_spec.clone(), pubkey, block_number, base_fee).unwrap();

        let pol_tx2_envelope =
            create_pol_transaction(chain_spec, pubkey, block_number, base_fee).unwrap();

        let pol_tx1 = match pol_tx1_envelope {
            BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        let pol_tx2 = match pol_tx2_envelope {
            BerachainTxEnvelope::Berachain(sealed_tx) => sealed_tx,
            _ => panic!("Expected PoL transaction"),
        };

        assert_eq!(
            pol_tx1.hash(),
            pol_tx2.hash(),
            "Identical PoL transactions should have identical hashes"
        );
    }
}
