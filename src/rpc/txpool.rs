//! `txpool_minPriorityFee`: exposes the node's configured priority-fee (tip) floor.

use alloy_primitives::U256;
use jsonrpsee::core::RpcResult;
use jsonrpsee_proc_macros::rpc;

/// `txpool_` RPC extension exposing the configured minimum priority fee.
#[rpc(server, namespace = "txpool")]
pub trait TxpoolMinPriorityFeeApi {
    /// Returns the configured `--txpool.minimum-priority-fee` in wei, or `null` if unset.
    ///
    /// Raw pool admission-policy value, mirroring what the pool builder is given; not a
    /// guarantee of per-transaction enforcement.
    #[method(name = "minPriorityFee")]
    fn min_priority_fee(&self) -> RpcResult<Option<U256>>;
}

/// Handler for [`TxpoolMinPriorityFeeApiServer`].
#[derive(Debug, Clone)]
pub struct TxpoolMinPriorityFeeHandler {
    /// Configured minimum priority fee in wei, mirroring `--txpool.minimum-priority-fee`.
    minimum_priority_fee: Option<u128>,
}

impl TxpoolMinPriorityFeeHandler {
    /// Creates a new handler from the configured minimum priority fee.
    pub const fn new(minimum_priority_fee: Option<u128>) -> Self {
        Self { minimum_priority_fee }
    }
}

impl TxpoolMinPriorityFeeApiServer for TxpoolMinPriorityFeeHandler {
    fn min_priority_fee(&self) -> RpcResult<Option<U256>> {
        Ok(self.minimum_priority_fee.map(U256::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_configured_fee() {
        let handler = TxpoolMinPriorityFeeHandler::new(Some(2_000_000_000));
        assert_eq!(handler.min_priority_fee().unwrap(), Some(U256::from(2_000_000_000u128)));
    }

    #[test]
    fn returns_null_when_unset() {
        let handler = TxpoolMinPriorityFeeHandler::new(None);
        assert_eq!(handler.min_priority_fee().unwrap(), None);
    }
}
