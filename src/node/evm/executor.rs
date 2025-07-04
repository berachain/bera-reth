use crate::{node::evm::config::BerachainEvmConfig, transaction::BerachainTxEnvelope};
use alloy_consensus::TxReceipt;
use reth::{
    providers::BlockExecutionResult,
    revm::{Inspector, State, context::result::ExecutionResult},
};
use reth_evm::{
    Database, EthEvmFactory, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded, OnStateHook,
    block::{
        BlockExecutionError, BlockExecutor, BlockExecutorFactory, BlockExecutorFor, CommitChanges,
        ExecutableTx, SystemCaller,
    },
    eth::EthBlockExecutionCtx,
};
use reth_evm_ethereum::RethReceiptBuilder;

#[derive(Debug)]
pub struct BerachainBlockExecutor<'a, Evm, Spec> {
    /// Context for block execution.
    pub ctx: EthBlockExecutionCtx<'a>,
    /// Inner EVM.
    evm: Evm,
    /// Utility to call system smart contracts.
    system_caller: SystemCaller<Spec>,
    /// Receipt builder.
    receipt_builder: RethReceiptBuilder,

    /// Receipts of executed transactions.
    receipts: Vec<RethReceiptBuilder>,
    /// Total gas used by transactions in this block.
    gas_used: u64,
}

impl<'a, Evm, Spec> BerachainBlockExecutor<'a, Evm, Spec>
where
    Spec: Clone,
{
    pub fn new(
        evm: Evm,
        ctx: EthBlockExecutionCtx<'a>,
        spec: Spec,
        receipt_builder: RethReceiptBuilder,
    ) -> Self {
        Self {
            evm,
            ctx,
            receipts: Vec::new(),
            gas_used: 0,
            system_caller: SystemCaller::new(spec.clone()),
            receipt_builder,
        }
    }
}

impl<'db, DB, E, Spec> BlockExecutor for BerachainBlockExecutor<'_, E, Spec>
where
    DB: Database + 'db,
    E: Evm<
            DB = &'db mut State<DB>,
            Tx: FromRecoveredTx<BerachainTxEnvelope> + FromTxWithEncoded<BerachainTxEnvelope>,
        >,
{
    type Transaction = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt;
    type Evm = E;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        todo!()
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        todo!()
    }

    fn finish(
        self,
    ) -> Result<(Self::Evm, BlockExecutionResult<Self::Receipt>), BlockExecutionError> {
        todo!()
    }

    fn set_state_hook(&mut self, hook: Option<Box<dyn OnStateHook>>) {
        todo!()
    }

    fn evm_mut(&mut self) -> &mut Self::Evm {
        todo!()
    }

    fn evm(&self) -> &Self::Evm {
        todo!()
    }
}

impl BlockExecutorFactory for BerachainEvmConfig {
    type EvmFactory = EthEvmFactory;
    type ExecutionCtx<'a> = EthBlockExecutionCtx<'a>;
    type Transaction = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt;

    fn evm_factory(&self) -> &Self::EvmFactory {
        todo!()
    }

    fn create_executor<'a, DB, I>(
        &'a self,
        evm: <Self::EvmFactory as EvmFactory>::Evm<&'a mut State<DB>, I>,
        ctx: Self::ExecutionCtx<'a>,
    ) -> impl BlockExecutorFor<'a, Self, DB, I>
    where
        DB: Database + 'a,
        I: Inspector<<Self::EvmFactory as EvmFactory>::Context<&'a mut State<DB>>> + 'a,
    {
        BerachainBlockExecutor::new(evm, ctx, &self.spec, self.receipt_builder)
    }
}
