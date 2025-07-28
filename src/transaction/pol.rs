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
