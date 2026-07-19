use crate::{
    chainspec::BerachainChainSpec,
    engine::validate_proposer_pubkey_prague1,
    evm::BerachainEvmFactory,
    hardforks::BerachainHardforks,
    node::evm::{
        block_context::BerachainBlockExecutionCtx, config::BerachainEvmConfig,
        error::BerachainExecutionError, receipt::BerachainReceiptBuilder,
    },
    transaction::{BerachainTxEnvelope, BerachainTxType, pol::create_pol_transaction},
};
use alloy_consensus::Transaction;
use alloy_eips::{Encodable2718, eip7685::Requests};
use alloy_evm::{RecoveredTx, block::state_changes::post_block_balance_increments};
use alloy_primitives::Bytes;
use reth::{
    chainspec::{EthereumHardfork, EthereumHardforks},
    providers::BlockExecutionResult,
    revm::{
        DatabaseCommit, Inspector,
        context::{
            Block as _,
            result::{ExecutionResult, Output, ResultAndState, SuccessReason},
        },
        database_interface::DatabaseCommitExt,
    },
};
use reth_evm::{
    Evm, EvmFactory, FromRecoveredTx, FromTxWithEncoded,
    block::{
        BlockExecutionError, BlockExecutor, BlockExecutorFactory, BlockValidationError,
        ExecutableTx, GasOutput, StateDB, SystemCaller, TxResult,
    },
    eth::{
        dao_fork, eip6110,
        receipt_builder::{ReceiptBuilder, ReceiptBuilderCtx},
        spec::EthExecutorSpec,
    },
};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct BerachainTxResult<H> {
    pub result: ResultAndState<H>,
    pub blob_gas_used: u64,
    pub tx_type: BerachainTxType,
}

impl<H: Send + 'static> TxResult for BerachainTxResult<H> {
    type HaltReason = H;
    fn result(&self) -> &ResultAndState<H> {
        &self.result
    }

    fn into_result(self) -> ResultAndState<H> {
        self.result
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

    /// Execute POL transaction as system call and manually capture receipt
    fn execute_pol_transaction_with_receipt(&mut self) -> Result<(), BlockExecutionError>
    where
        Evm: reth_evm::Evm,
        <Evm as reth_evm::Evm>::DB: DatabaseCommit,
    {
        let timestamp = self.evm.block().timestamp().saturating_to();

        // Validate proposer pubkey presence for Prague1
        validate_proposer_pubkey_prague1(&*self.spec, timestamp, self.ctx.prev_proposer_pubkey)?;

        // Check if Prague1 hardfork is active (after validation)
        if !self.spec.is_prague1_active_at_timestamp(timestamp) {
            return Ok(());
        }

        // This panic should never occur due to the above validation
        let prev_proposer_pubkey = self.ctx.prev_proposer_pubkey.unwrap();

        // Use shared POL transaction creation logic
        let base_fee = self.evm.block().basefee();
        let pol_envelope = create_pol_transaction(
            self.spec.clone(),
            prev_proposer_pubkey,
            self.evm.block().number(),
            base_fee,
            self.evm.block().gas_limit(),
        )?;
        let (caller_address, calldata, pol_distributor_address) =
            if let BerachainTxEnvelope::Berachain(pol_tx) = &pol_envelope {
                (pol_tx.from, pol_tx.input.clone(), pol_tx.to)
            } else {
                return Err(BerachainExecutionError::InvalidPolTransactionType.into());
            };

        // Execute as system call (maintains zero gas cost and unlimited gas)
        match self.evm.transact_system_call(
            caller_address,
            pol_distributor_address,
            calldata.clone(),
        ) {
            Ok(result_and_state) => {
                tracing::debug!(target: "executor", ?result_and_state, "POL transaction executed successfully");

                // Build receipt manually for the system call
                let receipt = self.receipt_builder.build_receipt(ReceiptBuilderCtx {
                    tx_type: BerachainTxType::Berachain,
                    evm: &self.evm,
                    result: result_and_state.result,
                    state: &result_and_state.state,
                    cumulative_gas_used: self.gas_used, // No gas consumed by system call
                });

                // Add receipt to block
                self.receipts.push(receipt);

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

impl<E> BlockExecutor for BerachainBlockExecutor<'_, E>
where
    E: Evm<
            DB: StateDB,
            Tx: FromRecoveredTx<BerachainTxEnvelope> + FromTxWithEncoded<BerachainTxEnvelope>,
        >,
{
    type Transaction = BerachainTxEnvelope;
    type Receipt = reth_ethereum_primitives::Receipt<BerachainTxType>;
    type Evm = E;
    type Result = BerachainTxResult<E::HaltReason>;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        self.system_caller.apply_blockhashes_contract_call(self.ctx.parent_hash, &mut self.evm)?;
        self.system_caller
            .apply_beacon_root_contract_call(self.ctx.parent_beacon_block_root, &mut self.evm)?;

        // Execute POL transaction and capture receipt
        self.execute_pol_transaction_with_receipt()?;
        Ok(())
    }

    fn execute_transaction_without_commit(
        &mut self,
        tx: impl ExecutableTx<Self>,
    ) -> Result<Self::Result, BlockExecutionError> {
        let (tx_env, recovered) = tx.into_parts();
        let consensus_tx = recovered.tx();

        // For PoL txs, we simply populate a dummy result and state as it is ultimately ignored
        // during commit_transaction.
        if let BerachainTxEnvelope::Berachain(_) = consensus_tx {
            return Ok(BerachainTxResult {
                result: ResultAndState {
                    result: ExecutionResult::Success {
                        reason: SuccessReason::Stop,
                        gas: reth::revm::context::result::ResultGas::default(),
                        logs: Vec::new(),
                        output: Output::Call(Bytes::default()),
                    },
                    state: HashMap::default(),
                },
                blob_gas_used: 0,
                tx_type: BerachainTxType::Berachain,
            });
        }

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

    fn commit_transaction(&mut self, output: Self::Result) -> GasOutput {
        // Skip commit for POL transactions as it's already been applied in
        // apply_pre_execution_changes
        if output.tx_type == BerachainTxType::Berachain {
            return GasOutput::new(0);
        }

        let BerachainTxResult { result: ResultAndState { result, state }, blob_gas_used, tx_type } =
            output;

        let gas_used = result.tx_gas_used();

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

        GasOutput::new(gas_used)
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
            self.ctx.withdrawals.as_deref().map(|w| w.as_slice()),
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
            .increment_balances(balance_increments)
            .map_err(|_| BlockValidationError::IncrementBalanceFailed)?;

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
    type TxExecutionResult = BerachainTxResult<<BerachainEvmFactory as EvmFactory>::HaltReason>;
    type Executor<'a, DB: StateDB, I: Inspector<<BerachainEvmFactory as EvmFactory>::Context<DB>>> =
        BerachainBlockExecutor<'a, <BerachainEvmFactory as EvmFactory>::Evm<DB, I>>;

    fn evm_factory(&self) -> &Self::EvmFactory {
        &self.evm_factory
    }

    fn create_executor<'a, DB, I>(
        &'a self,
        evm: <Self::EvmFactory as EvmFactory>::Evm<DB, I>,
        ctx: Self::ExecutionCtx<'a>,
    ) -> Self::Executor<'a, DB, I>
    where
        DB: StateDB,
        I: Inspector<<Self::EvmFactory as EvmFactory>::Context<DB>>,
    {
        BerachainBlockExecutor::new(evm, ctx, self.spec.clone(), self.receipt_builder)
    }
}
