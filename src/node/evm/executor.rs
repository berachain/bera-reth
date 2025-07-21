use crate::{
    chainspec::BerachainChainSpec,
    engine::validate_proposer_pubkey_prague1,
    hardforks::BerachainHardforks,
    node::evm::{
        block_context::BerachainBlockExecutionCtx, config::BerachainEvmConfig,
        error::BerachainExecutionError, receipt::BerachainReceiptBuilder,
    },
    transaction::{BerachainTxEnvelope, BerachainTxType, pol::create_pol_transaction},
};
use alloy_consensus::Transaction;
use alloy_eips::{Encodable2718, eip7685::Requests};
use reth::{
    chainspec::{EthereumHardfork, EthereumHardforks},
    providers::BlockExecutionResult,
    revm::{
        DatabaseCommit, Inspector, State,
        context::result::{ExecutionResult, ResultAndState},
    },
};
use reth_evm::{
    Database, EthEvmFactory, Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded, OnStateHook,
    block::{
        BlockExecutionError, BlockExecutor, BlockExecutorFactory, BlockExecutorFor,
        BlockValidationError, CommitChanges, ExecutableTx, StateChangePostBlockSource,
        StateChangeSource, SystemCaller,
    },
    eth::{
        dao_fork, eip6110,
        receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
    },
    state_change::{balance_increment_state, post_block_balance_increments},
};
use std::{borrow::Cow, sync::Arc};

#[derive(Debug)]
pub struct BerachainBlockExecutor<'a, Evm> {
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
    /// Transaction index counter for validation.
    transaction_index: usize,
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
            system_caller: SystemCaller::new(spec.clone()),
            receipt_builder,
            transaction_index: 0,
        }
    }

    /// Execute POL transaction as system call and manually capture receipt
    fn execute_pol_transaction_with_receipt(&mut self) -> Result<(), BlockExecutionError>
    where
        Evm: reth_evm::Evm,
        <Evm as reth_evm::Evm>::DB: DatabaseCommit,
    {
        use alloy_eips::eip7002::SYSTEM_ADDRESS;
        use reth::revm::DatabaseCommit;
        use reth_evm::block::StateChangeSource;

        let timestamp = self.evm.block().timestamp.saturating_to();

        // Validate proposer pubkey presence for Prague1
        validate_proposer_pubkey_prague1(&*self.spec, timestamp, self.ctx.prev_proposer_pubkey)?;

        // Check if Prague1 hardfork is active (after validation)
        if !self.spec.is_prague1_active_at_timestamp(timestamp) {
            return Ok(());
        }

        let prev_proposer_pubkey = self.ctx.prev_proposer_pubkey.unwrap();

        // Use shared POL transaction creation logic
        let base_fee = self.evm.block().basefee;
        let pol_envelope = create_pol_transaction(
            self.spec.clone(),
            prev_proposer_pubkey,
            self.evm.block().number,
            base_fee,
        )?;
        let (calldata, pol_distributor_address) =
            if let BerachainTxEnvelope::Berachain(pol_tx) = &pol_envelope {
                (pol_tx.input.clone(), pol_tx.to)
            } else {
                return Err(BerachainExecutionError::InvalidPolTransactionType.into());
            };

        // Execute as system call (maintains zero gas cost and unlimited gas)
        match self.evm.transact_system_call(
            SYSTEM_ADDRESS,
            pol_distributor_address,
            calldata.clone(),
        ) {
            Ok(result_and_state) => {
                tracing::info!(target: "executor", ?result_and_state, "POL transaction executed successfully");

                // Use the already-created POL envelope for receipt generation

                // Build receipt manually for the system call
                let receipt = self.receipt_builder.build_receipt(ReceiptBuilderCtx {
                    tx: &pol_envelope,
                    evm: &self.evm,
                    result: result_and_state.result,
                    state: &result_and_state.state,
                    cumulative_gas_used: self.gas_used, // No gas consumed by system call
                });

                // Add receipt to block
                self.receipts.push(receipt);

                // Notify system caller of state changes from system call
                self.system_caller.on_state(
                    StateChangeSource::Transaction(0), /* POL is always the first transaction
                                                        * (index 0) */
                    &result_and_state.state,
                );

                // Commit the POL transaction state changes to the database
                self.evm.db_mut().commit(result_and_state.state);

                tracing::debug!(target: "executor", "POL transaction state changes committed to database");

                Ok(())
            }
            Err(e) => {
                tracing::error!(target: "executor", %e, "POL system call execution failed");
                Err(BlockExecutionError::other(e))
            }
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

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        // Set state clear flag if the block is after the Spurious Dragon hardfork.
        let state_clear_flag =
            self.spec.is_spurious_dragon_active_at_block(self.evm.block().number.saturating_to());
        self.evm.db_mut().set_state_clear_flag(state_clear_flag);

        self.system_caller.apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
        self.system_caller
            .apply_beacon_root_contract_call(self.ctx.parent_beacon_block_root, &mut self.evm)?;

        // Execute POL transaction and capture receipt
        self.execute_pol_transaction_with_receipt()?;
        Ok(())
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutableTx<Self>,
        f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        let is_prague1_active =
            self.spec.is_prague1_active_at_timestamp(self.evm.block().timestamp.saturating_to());

        // Check if this is a POL transaction - skip validation since it's already executed as
        // systemcall. We check that the transaction is in the correct index, i.e. first of the
        // block as part of the BerachainBeaconConsensus.
        if let BerachainTxEnvelope::Berachain(_) = tx.tx() {
            // POL transactions are executed in apply_pre_execution_changes() as system calls
            // During block validation, we just return 0 gas used and skip re-execution

            // Validate that POL transaction is the first transaction in the block
            if self.transaction_index != 0 {
                tracing::error!(
                    target: "executor",
                    transaction_index = self.transaction_index,
                    "POL transaction found at incorrect index - must be first transaction"
                );
                return Err(BerachainExecutionError::PolTransactionInvalidIndex {
                    expected_index: 0,
                    actual_index: self.transaction_index,
                }
                .into());
            }

            // Ensure we are after Prague1 hardfork activation
            if !is_prague1_active {
                tracing::error!(
                    target: "executor",
                    "POL transaction found before Prague1 activation - invalid block"
                );
                return Err(BerachainExecutionError::PolTransactionBeforePragueOne.into());
            }

            // Additional validation: Verify POL transaction matches expected synthetic transaction
            // Create the canonical POL transaction and compare hashes
            let timestamp = self.evm.block().timestamp.saturating_to();
            validate_proposer_pubkey_prague1(
                &*self.spec,
                timestamp,
                self.ctx.prev_proposer_pubkey,
            )?;
            let prev_proposer_pubkey = self.ctx.prev_proposer_pubkey.unwrap();
            let base_fee = self.evm.block().basefee;
            let expected_pol_envelope = match create_pol_transaction(
                self.spec.clone(),
                prev_proposer_pubkey,
                self.evm.block().number,
                base_fee,
            ) {
                Ok(envelope) => envelope,
                Err(e) => {
                    tracing::error!(target: "executor", %e, "Failed to create canonical POL transaction for validation");
                    return Err(e);
                }
            };

            // Compare transaction hashes - this validates the entire transaction shape
            let received_tx_hash = tx.tx().trie_hash();
            let expected_tx_hash = expected_pol_envelope.trie_hash();

            if received_tx_hash != expected_tx_hash {
                tracing::error!(
                    target: "executor",
                    received_hash = ?received_tx_hash,
                    expected_hash = ?expected_tx_hash,
                    "POL transaction hash mismatch - transaction shape is invalid"
                );
                return Err(BerachainExecutionError::PolTransactionHashMismatch {
                    received_hash: received_tx_hash,
                    expected_hash: expected_tx_hash,
                }
                .into());
            }

            tracing::debug!(target: "executor", "POL transaction validation passed - skipping re-execution");

            // Increment transaction index counter for validation
            self.transaction_index += 1;

            return Ok(Some(0));
        }

        // TODO: This check is disabled as technically, the transaction index needs to be checked
        // during block assembly, since that's when the PoL Tx is inserted.
        // if is_prague1_active && self.transaction_index == 0 {
        //     // In Prague1 blocks, the first transaction MUST be a POL transaction
        //     tracing::error!(
        //         target: "executor",
        //         "First transaction in Prague1 block must be a POL transaction"
        //     );
        //     return Err(BerachainExecutionError::MissingPolTransactionAtIndex0.into());
        // }

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

        // Increment transaction index counter for validation
        self.transaction_index += 1;

        Ok(Some(gas_used))
    }

    fn finish(
        mut self,
    ) -> Result<
        (Self::Evm, BlockExecutionResult<<BerachainReceiptBuilder as ReceiptBuilder>::Receipt>),
        BlockExecutionError,
    > {
        let requests = if self
            .spec
            .is_prague_active_at_timestamp(self.evm.block().timestamp.saturating_to())
        {
            // Collect all EIP-6110 deposits
            let deposit_requests =
                eip6110::parse_deposits_from_receipts(&self.spec, &self.receipts)?;

            let mut requests = Requests::default();

            if !deposit_requests.is_empty() {
                requests.push_request_with_type(eip6110::DEPOSIT_REQUEST_TYPE, deposit_requests);
            }

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
            .transitions_at_block(self.evm.block().number.saturating_to())
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
            BlockExecutionResult { receipts: self.receipts, requests, gas_used: self.gas_used },
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
}

impl BlockExecutorFactory for BerachainEvmConfig {
    type EvmFactory = EthEvmFactory;
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
