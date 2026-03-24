use alloy_consensus::crypto::RecoveryError;
use alloy_eips::eip4895::Withdrawals;
use alloy_primitives::{B256, Bloom, Bytes};
use alloy_rpc_types_engine::PayloadId;

pub trait FlashblockPayloadBase: Clone + Send + Sync + 'static {
    fn parent_hash(&self) -> B256;
    fn block_number(&self) -> u64;
    fn timestamp(&self) -> u64;
}

pub trait FlashblockDiff: Clone + Send + Sync + 'static {
    fn block_hash(&self) -> B256;
    fn state_root(&self) -> B256;
    fn gas_used(&self) -> u64;
    fn logs_bloom(&self) -> &Bloom;
    fn receipts_root(&self) -> B256;
    fn transactions_raw(&self) -> &[Bytes];

    fn withdrawals(&self) -> Option<&Withdrawals> {
        None
    }

    fn withdrawals_root(&self) -> Option<B256> {
        None
    }
}

pub trait FlashblockPayload:
    Clone + Send + Sync + 'static + for<'de> serde::Deserialize<'de>
{
    type Base: FlashblockPayloadBase;
    type Diff: FlashblockDiff;
    type SignedTx: reth_primitives_traits::SignedTransaction;

    fn index(&self) -> u64;
    fn payload_id(&self) -> PayloadId;
    fn base(&self) -> Option<&Self::Base>;
    fn diff(&self) -> &Self::Diff;
    fn block_number(&self) -> u64;

    fn recover_transactions(
        &self,
    ) -> impl Iterator<
        Item = Result<
            alloy_eips::eip2718::WithEncoded<reth_primitives_traits::Recovered<Self::SignedTx>>,
            RecoveryError,
        >,
    >;
}
