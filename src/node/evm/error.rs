use alloy_primitives::{Address, B256};
use reth_evm::block::BlockExecutionError;

/// Berachain-specific execution errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BerachainExecutionError {
    /// Previous proposer public key is required for Prague1 hardfork.
    #[error("Previous proposer public key is required for Prague1 hardfork")]
    MissingProposerPubkey,
    /// Previous proposer public key is not allowed before Prague1 hardfork.
    #[error("Previous proposer public key is not allowed before Prague1 hardfork")]
    ProposerPubkeyNotAllowed,
    /// POL transaction found before Prague1 hardfork activation.
    #[error("POL transaction found before Prague1 hardfork activation")]
    PolTransactionBeforePragueOne,
    /// POL transaction receipts not found
    #[error("POL transaction receipts not found in block")]
    MissingPolReceipts,
    /// POL transaction hash mismatch during validation
    #[error("POL transaction hash mismatch: got {received_hash:?}, expected {expected_hash:?}")]
    PolTransactionHashMismatch { received_hash: B256, expected_hash: B256 },
    /// POL transaction found at incorrect index
    #[error("POL transaction found at index {actual_index}, expected index {expected_index}")]
    PolTransactionInvalidIndex { expected_index: usize, actual_index: usize },
    /// Missing POL transaction at index 0 in Prague1 block
    #[error("First transaction in Prague1 block must be a POL transaction")]
    MissingPolTransactionAtIndex0,
    /// Prague3: Block contains invalid ERC20 transfer from/to blocked address
    #[error(
        "Prague3 violation: Blocked address {blocked_address} can only send to rescue address or cannot receive transfers"
    )]
    Prague3BlockedAddressTransfer { blocked_address: Address },
    /// Prague3: Block contains InternalBalanceChanged event from BEX vault
    #[error("Prague3 violation: InternalBalanceChanged event from BEX vault {vault_address}")]
    Prague3BexVaultEvent { vault_address: Address },
    /// Prague3: Block contains ERC20 transfer from/to BEX vault
    #[error("Prague3 violation: ERC20 transfer from/to BEX vault {vault_address}")]
    Prague3BexVaultTransfer { vault_address: Address },
}

impl BerachainExecutionError {
    /// Convert to BlockExecutionError for compatibility with reth's error handling.
    pub fn into_block_execution_error(self) -> BlockExecutionError {
        BlockExecutionError::other(self)
    }
}

impl From<BerachainExecutionError> for BlockExecutionError {
    fn from(err: BerachainExecutionError) -> Self {
        err.into_block_execution_error()
    }
}
