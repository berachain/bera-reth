use crate::primitives::{BerachainHeader, header::BlsPublicKey};
use alloy_eips::eip4895::Withdrawals;
use alloy_primitives::B256;
use std::borrow::Cow;

/// Context for Berachain block execution.
#[derive(Debug, Clone)]
pub struct BerachainBlockExecutionCtx<'a> {
    /// The parent block hash
    pub parent_hash: B256,
    /// The parent beacon block root
    pub parent_beacon_block_root: Option<B256>,
    /// The block ommers (uncle blocks)
    pub ommers: &'a [BerachainHeader],
    /// The block withdrawals
    pub withdrawals: Option<Cow<'a, Withdrawals>>,
    /// Previous proposer public key.
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
}
