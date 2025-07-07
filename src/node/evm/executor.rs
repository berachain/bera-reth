use crate::{
    chainspec::BerachainChainSpec,
    node::evm::{config::BerachainEvmConfig, receipt::BerachainReceiptBuilder},
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::{Transaction, TxReceipt};
use alloy_eips::Encodable2718;
use reth::{
    chainspec::EthereumHardforks,
    providers::BlockExecutionResult,
    revm::{
        DatabaseCommit, Inspector, State,
        context::result::{ExecutionResult, ResultAndState},
    },
};
use reth_evm::{
    ConfigureEvm, Database, EthEvmFactory, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded,
    OnStateHook,
    block::{
        BlockExecutionError, BlockExecutor, BlockExecutorFactory, BlockExecutorFor,
        BlockValidationError, CommitChanges, ExecutableTx, StateChangeSource, SystemCaller,
    },
    eth::{
        EthBlockExecutionCtx,
        receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
    },
};
use reth_evm_ethereum::RethReceiptBuilder;

#[derive(Debug)]
pub struct BerachainBlockExecutor<'a, Evm, Spec> {
    spec: Spec,
    /// Context for block execution.
    pub ctx: EthBlockExecutionCtx<'a>,
    /// Inner EVM.
    evm: Evm,
    /// Utility to call system smart contracts.
    system_caller: SystemCaller<Spec>,
    /// Receipt builder.
    receipt_builder: BerachainReceiptBuilder,

    /// Receipts of executed transactions.
    receipts: Vec<<BerachainReceiptBuilder as ReceiptBuilder>::Receipt>,
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
        receipt_builder: BerachainReceiptBuilder,
    ) -> Self {
        Self {
            spec: spec.clone(),
            evm,
            ctx,
            receipts: Vec::new(),
            gas_used: 0,
            system_caller: SystemCaller::new(spec.clone()),
            receipt_builder,
        }
    }
}

impl<'db, DB, E, Spec: EthereumHardforks> BlockExecutor for BerachainBlockExecutor<'_, E, Spec>
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
        // Set state clear flag if the block is after the Spurious Dragon hardfork.
        let state_clear_flag =
            self.spec.is_spurious_dragon_active_at_block(self.evm.block().number.saturating_to());
        self.evm.db_mut().set_state_clear_flag(state_clear_flag);

        self.system_caller.apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
        self.system_caller
            .apply_beacon_root_contract_call(self.ctx.parent_beacon_block_root, &mut self.evm)?;
        Ok(())
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        // The sum of the transaction's gas limit, Tg, and the gas utilized in this block prior,
        // must be no greater than the block's gasLimit.
        let block_available_gas = self.evm.block().gas_limit - self.gas_used;

        if tx.tx().gas_limit() > block_available_gas {
            return Err(BlockValidationError::TransactionGasLimitMoreThanAvailableBlockGas {
                transaction_gas_limit: tx.tx().gas_limit(),
                block_available_gas,
            }
            .into());
        }

        // Execute transaction.
        let ResultAndState { result, state } = self
            .evm
            .transact(tx)
            .map_err(|err| BlockExecutionError::evm(err, tx.tx().trie_hash()))?;

        if !f(&result).should_commit() {
            return Ok(None);
        }

        self.system_caller.on_state(StateChangeSource::Transaction(self.receipts.len()), &state);

        let gas_used = result.gas_used();

        // append gas used
        self.gas_used += gas_used;

        // Push transaction changeset and calculate header bloom filter for receipt.
        self.receipts.push(self.receipt_builder.build_receipt(ReceiptBuilderCtx {
            tx: tx.tx(),
            evm: &self.evm,
            result,
            state: &state,
            cumulative_gas_used: self.gas_used,
        }));

        // Commit the state changes.
        self.evm.db_mut().commit(state);

        Ok(Some(gas_used))
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
        &mut self.evm
    }

    fn evm(&self) -> &Self::Evm {
        &self.evm
    }
}

impl BlockExecutorFactory for BerachainEvmConfig {
    type EvmFactory = EthEvmFactory;
    type ExecutionCtx<'a> = EthBlockExecutionCtx<'a>;
    type Transaction = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt;

    fn evm_factory(&self) -> &Self::EvmFactory {
        &self.evm_factory
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
