use crate::{
    chainspec::BerachainChainSpec, primitives::header::BlsPublicKey,
    transaction::BerachainTxEnvelope,
};
use alloy_primitives::U256;
use reth::revm::{handler::SYSTEM_ADDRESS, primitives::eip7825};
use reth_chainspec::EthChainSpec;
use reth_evm::block::{BlockExecutionError, InternalBlockExecutionError};
use std::sync::Arc;

/// Create a POL transaction with the given validator pubkey
/// This is the canonical POL transaction creation logic used by both executor and assembler
pub fn create_pol_transaction(
    chain_spec: Arc<BerachainChainSpec>,
    prev_proposer_pubkey: BlsPublicKey,
    block_number: U256,
    base_fee: u64,
) -> Result<BerachainTxEnvelope, BlockExecutionError> {
    use crate::transaction::PoLTx;
    use alloy_primitives::{Bytes, Sealed};
    use alloy_sol_macro::sol;
    use alloy_sol_types::SolCall;

    // Construct ABI-encoded calldata
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

    // Create POL transaction
    let pol_tx = PoLTx {
        chain_id: chain_spec.chain_id(),
        from: SYSTEM_ADDRESS,
        to: chain_spec.pol_contract(),
        input: Bytes::from(calldata),
        nonce,
        gas_limit: eip7825::TX_GAS_LIMIT_CAP, // this is the env value used in revm for system calls
        gas_price: base_fee.into(),           /* gas price is set to the base fee for RPC
                                               * compatability reasons */
    };

    // Wrap in transaction envelope and calculate proper hash
    Ok(BerachainTxEnvelope::Berachain(Sealed::new(pol_tx)))
}
