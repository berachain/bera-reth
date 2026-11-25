use crate::{
    chainspec::BerachainChainSpec,
    engine::validate_proposer_pubkey_prague1,
    evm::BerachainEvmFactory,
    node::evm::{
        block_context::BerachainBlockExecutionCtx, config::BerachainEvmConfig,
        receipt::BerachainReceiptBuilder,
    },
    transaction::{BerachainTxEnvelope, BerachainTxType},
};
use alloy_consensus::Transaction;
use alloy_eips::{Encodable2718, eip7685::Requests};
use alloy_evm::{
    RecoveredTx,
    block::state_changes::{balance_increment_state, post_block_balance_increments},
};
use reth::{
    chainspec::{EthereumHardfork, EthereumHardforks},
    providers::BlockExecutionResult,
    revm::{
        DatabaseCommit, Inspector, State,
        context::{Block as _, result::ResultAndState},
        database_interface::DatabaseCommitExt,
    },
};
use reth_evm::{
    Database, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded, OnStateHook,
    block::{
        BlockExecutionError, BlockExecutor, BlockExecutorFactory, BlockExecutorFor,
        BlockValidationError, ExecutableTx, StateChangePostBlockSource, StateChangeSource,
        SystemCaller, TxResult,
    },
    eth::{
        dao_fork, eip6110,
        receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
        spec::EthExecutorSpec,
    },
};
use std::{borrow::Cow, sync::Arc};

#[derive(Debug)]
pub struct BerachainTxResult<H> {
    pub result: ResultAndState<H>,
    pub blob_gas_used: u64,
    pub tx_type: BerachainTxType,
}

impl<H> TxResult for BerachainTxResult<H> {
    type HaltReason = H;
    fn result(&self) -> &ResultAndState<H> {
        &self.result
    }
}

#[derive(Debug)]
pub struct BerachainBlockExecutor<'a, Evm> {
    /// Berachain chain specification.
    spec: Arc<BerachainChainSpec>,
    /// Context for block execution.
    pub ctx: BerachainBlockExecutionCtx<'a>,
    /// Inner EVM.
    evm: Evm,
    /// Utility to call system smart contracts.
    system_caller: SystemCaller<Arc<BerachainChainSpec>>,
    /// Receipt builder.
    receipt_builder: BerachainReceiptBuilder,

    /// Receipts of executed transactions.
    receipts: Vec<<BerachainReceiptBuilder as ReceiptBuilder>::Receipt>,
    /// Total gas used by transactions in this block.
    gas_used: u64,
    /// Total blob gas used by blob transactions in this block.
    blob_gas_used: u64,
}

impl<'a, Evm> BerachainBlockExecutor<'a, Evm> {
    pub fn new(
        evm: Evm,
        ctx: BerachainBlockExecutionCtx<'a>,
        spec: Arc<BerachainChainSpec>,
        receipt_builder: BerachainReceiptBuilder,
    ) -> Self {
        Self {
            spec: spec.clone(),
            evm,
            ctx,
            receipts: Vec::new(),
            gas_used: 0,
            blob_gas_used: 0,
            system_caller: SystemCaller::new(spec.clone()),
            receipt_builder,
        }
    }
}

impl<'db, DB, E> BlockExecutor for BerachainBlockExecutor<'_, E>
where
    DB: Database + 'db,
    E: Evm<
            DB = &'db mut State<DB>,
            Tx: FromRecoveredTx<BerachainTxEnvelope> + FromTxWithEncoded<BerachainTxEnvelope>,
        >,
{
    type Transaction = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt<BerachainTxType>;
    type Evm = E;
    type Result = BerachainTxResult<E::HaltReason>;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        // Set state clear flag if the block is after the Spurious Dragon hardfork.
        let state_clear_flag =
            self.spec.is_spurious_dragon_active_at_block(self.evm.block().number().saturating_to());
        self.evm.db_mut().set_state_clear_flag(state_clear_flag);

        self.system_caller.apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
        self.system_caller
            .apply_beacon_root_contract_call(self.ctx.parent_beacon_block_root, &mut self.evm)?;

        // Enforce prev_proposer_pubkey presence rules for Prague1.
        let timestamp = self.evm.block().timestamp().saturating_to();
        validate_proposer_pubkey_prague1(&*self.spec, timestamp, self.ctx.prev_proposer_pubkey)?;
        Ok(())
    }

    fn execute_transaction_without_commit(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<Self::Result, BlockExecutionError> {
        let (tx_env, recovered) = tx.into_parts();
        let consensus_tx = recovered.tx();

        // The sum of the transaction's gas limit, Tg, and the gas utilized in this block prior,
        // must be no greater than the block's gasLimit.
        let block_available_gas = self.evm.block().gas_limit() - self.gas_used;

        if consensus_tx.gas_limit() > block_available_gas {
            return Err(BlockValidationError::TransactionGasLimitMoreThanAvailableBlockGas {
                transaction_gas_limit: consensus_tx.gas_limit(),
                block_available_gas,
            }
            .into());
        }

        let blob_gas_used = consensus_tx.blob_gas_used().unwrap_or_default();
        let tx_type = consensus_tx.tx_type();
        let tx_hash = consensus_tx.trie_hash();

        // Execute transaction and return the result
        let result =
            self.evm.transact_raw(tx_env).map_err(|err| BlockExecutionError::evm(err, tx_hash))?;

        Ok(BerachainTxResult { result, blob_gas_used, tx_type })
    }

    fn commit_transaction(&mut self, output: Self::Result) -> Result<u64, BlockExecutionError> {
        let BerachainTxResult { result: ResultAndState { result, state }, blob_gas_used, tx_type } =
            output;

        self.system_caller.on_state(StateChangeSource::Transaction(self.receipts.len()), &state);

        let gas_used = result.gas_used();

        // append gas used
        self.gas_used += gas_used;

        // only determine cancun fields when active
        if self.spec.is_cancun_active_at_timestamp(self.evm.block().timestamp().saturating_to()) {
            self.blob_gas_used = self.blob_gas_used.saturating_add(blob_gas_used);
        }

        // Push transaction changeset and calculate header bloom filter for receipt.
        self.receipts.push(self.receipt_builder.build_receipt(ReceiptBuilderCtx {
            tx_type,
            evm: &self.evm,
            result,
            state: &state,
            cumulative_gas_used: self.gas_used,
        }));

        // Commit the state changes.
        self.evm.db_mut().commit(state);

        Ok(gas_used)
    }

    fn finish(
        mut self,
    ) -> Result<
        (Self::Evm, BlockExecutionResult<<BerachainReceiptBuilder as ReceiptBuilder>::Receipt>),
        BlockExecutionError,
    > {
        let timestamp = self.evm.block().timestamp().saturating_to();
        let requests = if self.spec.is_prague_active_at_timestamp(timestamp) {
            let mut requests = Requests::default();

            // EIP-6110 deposit requests are sourced from the execution layer only once Osaka is
            // active. Before Osaka the consensus layer still ingests deposits from the deposit
            // contract logs, so emitting them here would double count them.
            if self.spec.is_osaka_active_at_timestamp(timestamp) {
                let deposit_contract = self
                    .spec
                    .deposit_contract_address()
                    .unwrap_or(eip6110::MAINNET_DEPOSIT_CONTRACT_ADDRESS);
                let deposit_requests = crate::deposits::parse_deposits_from_receipts(
                    deposit_contract,
                    &self.receipts,
                )?;

                if !deposit_requests.is_empty() {
                    requests
                        .push_request_with_type(eip6110::DEPOSIT_REQUEST_TYPE, deposit_requests);
                }
            }

            // EIP-7002 withdrawal and EIP-7251 consolidation requests are part of Prague and must
            // run their system-contract calls every block from Prague onward. Gating these behind
            // Osaka changes the post-state of pre-Osaka blocks (e.g. the excess-requests slot is
            // never cleared), which breaks re-execution of historical blocks.
            requests.extend(self.system_caller.apply_post_execution_changes(&mut self.evm)?);
            requests
        } else {
            Requests::default()
        };

        let mut balance_increments = post_block_balance_increments(
            &self.spec,
            self.evm.block(),
            self.ctx.ommers,
            self.ctx.withdrawals.as_deref(),
        );

        // Irregular state change at Ethereum DAO hardfork
        if self
            .spec
            .ethereum_fork_activation(EthereumHardfork::Dao)
            .transitions_at_block(self.evm.block().number().saturating_to())
        {
            // drain balances from hardcoded addresses.
            let drained_balance: u128 = self
                .evm
                .db_mut()
                .drain_balances(dao_fork::DAO_HARDFORK_ACCOUNTS)
                .map_err(|_| BlockValidationError::IncrementBalanceFailed)?
                .into_iter()
                .sum();

            // return balance to DAO beneficiary.
            *balance_increments.entry(dao_fork::DAO_HARDFORK_BENEFICIARY).or_default() +=
                drained_balance;
        }
        // increment balances
        self.evm
            .db_mut()
            .increment_balances(balance_increments.clone())
            .map_err(|_| BlockValidationError::IncrementBalanceFailed)?;

        // call state hook with changes due to balance increments.
        self.system_caller.try_on_state_with(|| {
            balance_increment_state(&balance_increments, self.evm.db_mut()).map(|state| {
                (
                    StateChangeSource::PostBlock(StateChangePostBlockSource::BalanceIncrements),
                    Cow::Owned(state),
                )
            })
        })?;

        Ok((
            self.evm,
            BlockExecutionResult {
                receipts: self.receipts,
                requests,
                gas_used: self.gas_used,
                blob_gas_used: self.blob_gas_used,
            },
        ))
    }

    fn set_state_hook(&mut self, hook: Option<Box<dyn OnStateHook>>) {
        self.system_caller.with_state_hook(hook);
    }

    fn evm_mut(&mut self) -> &mut Self::Evm {
        &mut self.evm
    }

    fn evm(&self) -> &Self::Evm {
        &self.evm
    }

    fn receipts(&self) -> &[Self::Receipt] {
        &self.receipts
    }
}

impl BlockExecutorFactory for BerachainEvmConfig {
    type EvmFactory = BerachainEvmFactory;
    type ExecutionCtx<'a> = BerachainBlockExecutionCtx<'a>;
    type Transaction = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt<BerachainTxType>;

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
        BerachainBlockExecutor::new(evm, ctx, self.spec.clone(), self.receipt_builder)
    }
}
