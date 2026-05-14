use crate::transaction::POL_TX_TYPE;
use alloy_primitives::{Address, Bytes, TxKind};
use reth::revm::{
    Context, ExecuteEvm, InspectEvm, InspectSystemCallEvm, Inspector, MainBuilder, MainContext,
    SystemCallEvm,
    context::{
        BlockEnv, CfgEnv, Evm as RevmEvm, Transaction as TxEnvTransaction, TxEnv,
        result::{EVMError, HaltReason, ResultAndState},
    },
    context_interface::result::ExecutionResult,
    handler::{EthFrame, EthPrecompiles, PrecompileProvider, instructions::EthInstructions},
    inspector::NoOpInspector,
    interpreter::{InterpreterResult, interpreter::EthInterpreter},
    precompile::{PrecompileSpecId, Precompiles},
    primitives::hardfork::SpecId,
};
use reth_evm::{
    Database, Evm, EvmEnv, EvmFactory, eth::EthEvmContext, precompiles::PrecompilesMap,
};
use std::ops::{Deref, DerefMut};

/// Helper builder to construct `BerachainEvm` instances in a unified way.
#[derive(Debug)]
pub struct BerachainEvmBuilder<DB: Database, I = NoOpInspector> {
    db: DB,
    block_env: BlockEnv,
    cfg_env: CfgEnv,
    inspector: I,
    inspect: bool,
    precompiles: Option<PrecompilesMap>,
}

impl<DB: Database> BerachainEvmBuilder<DB, NoOpInspector> {
    /// Creates a builder from the provided `EvmEnv` and database.
    pub fn new(db: DB, env: EvmEnv) -> Self {
        Self {
            db,
            block_env: env.block_env,
            cfg_env: env.cfg_env,
            inspector: NoOpInspector {},
            inspect: false,
            precompiles: None,
        }
    }
}

impl<DB: Database, I> BerachainEvmBuilder<DB, I> {
    /// Sets a custom inspector
    pub fn inspector<J>(self, inspector: J) -> BerachainEvmBuilder<DB, J> {
        BerachainEvmBuilder {
            db: self.db,
            block_env: self.block_env,
            cfg_env: self.cfg_env,
            inspector,
            inspect: self.inspect,
            precompiles: self.precompiles,
        }
    }

    /// Sets a custom inspector and enables invoking it during transaction execution.
    pub fn activate_inspector<J>(self, inspector: J) -> BerachainEvmBuilder<DB, J> {
        self.inspector(inspector).inspect()
    }

    /// Sets whether to invoke the inspector during transaction execution.
    pub fn set_inspect(mut self, inspect: bool) -> Self {
        self.inspect = inspect;
        self
    }

    /// Enables invoking the inspector during transaction execution.
    pub fn inspect(self) -> Self {
        self.set_inspect(true)
    }

    /// Overrides the precompiles map. If not provided, it will be derived from the `SpecId` in
    /// `CfgEnv`.
    pub fn precompiles(mut self, precompiles: PrecompilesMap) -> Self {
        self.precompiles = Some(precompiles);
        self
    }

    /// Builds the `BerachainEvm` instance.
    pub fn build(self) -> BerachainEvm<DB, I, PrecompilesMap>
    where
        I: Inspector<EthEvmContext<DB>>,
    {
        let precompiles = match self.precompiles {
            Some(p) => p,
            None => PrecompilesMap::from_static(Precompiles::new(PrecompileSpecId::from_spec_id(
                self.cfg_env.spec,
            ))),
        };

        let inner = Context::mainnet()
            .with_block(self.block_env)
            .with_cfg(self.cfg_env)
            .with_db(self.db)
            .build_mainnet_with_inspector(self.inspector)
            .with_precompiles(precompiles);

        BerachainEvm { inner, inspect: self.inspect }
    }
}

/// Berachain EVM implementation.
///
/// This is a wrapper type around the `revm` ethereum evm with optional [`Inspector`] (tracing)
/// support. [`Inspector`] support is configurable at runtime because it's part of the underlying
/// [`RevmEvm`] type.
#[expect(missing_debug_implementations)]
pub struct BerachainEvm<DB: Database, I, PRECOMPILE = EthPrecompiles> {
    inner: RevmEvm<
        EthEvmContext<DB>,
        I,
        EthInstructions<EthInterpreter, EthEvmContext<DB>>,
        PRECOMPILE,
        EthFrame,
    >,
    inspect: bool,
}

impl<DB: Database, I, PRECOMPILE> BerachainEvm<DB, I, PRECOMPILE> {
    /// Creates a new Berachain EVM instance.
    ///
    /// The `inspect` argument determines whether the configured [`Inspector`] of the given
    /// [`RevmEvm`] should be invoked on [`Evm::transact`].
    pub const fn new(
        evm: RevmEvm<
            EthEvmContext<DB>,
            I,
            EthInstructions<EthInterpreter, EthEvmContext<DB>>,
            PRECOMPILE,
            EthFrame,
        >,
        inspect: bool,
    ) -> Self {
        Self { inner: evm, inspect }
    }

    /// Consumes self and return the inner EVM instance.
    pub fn into_inner(
        self,
    ) -> RevmEvm<
        EthEvmContext<DB>,
        I,
        EthInstructions<EthInterpreter, EthEvmContext<DB>>,
        PRECOMPILE,
        EthFrame,
    > {
        self.inner
    }

    /// Provides a reference to the EVM context.
    pub const fn ctx(&self) -> &EthEvmContext<DB> {
        &self.inner.ctx
    }

    /// Provides a mutable reference to the EVM context.
    pub fn ctx_mut(&mut self) -> &mut EthEvmContext<DB> {
        &mut self.inner.ctx
    }
}

impl<DB: Database, I, PRECOMPILE> Deref for BerachainEvm<DB, I, PRECOMPILE> {
    type Target = EthEvmContext<DB>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.ctx()
    }
}

impl<DB: Database, I, PRECOMPILE> DerefMut for BerachainEvm<DB, I, PRECOMPILE> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx_mut()
    }
}

impl<DB, I, PRECOMPILE> Evm for BerachainEvm<DB, I, PRECOMPILE>
where
    DB: Database,
    I: Inspector<EthEvmContext<DB>>,
    PRECOMPILE: PrecompileProvider<EthEvmContext<DB>, Output = InterpreterResult>,
{
    type DB = DB;
    type Tx = TxEnv;
    type Error = EVMError<DB::Error>;
    type HaltReason = HaltReason;
    type Spec = SpecId;
    type Precompiles = PRECOMPILE;
    type Inspector = I;
    type BlockEnv = BlockEnv;

    fn block(&self) -> &BlockEnv {
        &self.block
    }

    fn chain_id(&self) -> u64 {
        self.cfg.chain_id
    }

    fn transact_raw(
        &mut self,
        tx: Self::Tx,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        if TxEnvTransaction::tx_type(&tx) == POL_TX_TYPE {
            return match tx.kind {
                TxKind::Create => {
                    Err(EVMError::Custom("POL Create transactions are unsupported".into()))
                }
                TxKind::Call(to) => {
                    let mut result = self.transact_system_call(tx.caller, to, tx.data)?;
                    // Set gas_used to 0 for POL transactions
                    result.result = match result.result {
                        ExecutionResult::Success { reason, gas_refunded, logs, output, .. } => {
                            ExecutionResult::Success {
                                reason,
                                gas_used: 0,
                                gas_refunded,
                                logs,
                                output,
                            }
                        }
                        other => other,
                    };
                    Ok(result)
                }
            };
        }
        if self.inspect { self.inner.inspect_tx(tx) } else { self.inner.transact(tx) }
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        if self.inspect {
            self.inner.inspect_system_call_with_caller(caller, contract, data)
        } else {
            self.inner.system_call_with_caller(caller, contract, data)
        }
    }

    fn finish(self) -> (Self::DB, EvmEnv<Self::Spec>) {
        let Context { block: block_env, cfg: cfg_env, journaled_state, .. } = self.inner.ctx;

        (journaled_state.database, EvmEnv { block_env, cfg_env })
    }

    fn set_inspector_enabled(&mut self, enabled: bool) {
        self.inspect = enabled;
    }

    fn components(&self) -> (&Self::DB, &Self::Inspector, &Self::Precompiles) {
        (&self.inner.ctx.journaled_state.database, &self.inner.inspector, &self.inner.precompiles)
    }

    fn components_mut(&mut self) -> (&mut Self::DB, &mut Self::Inspector, &mut Self::Precompiles) {
        (
            &mut self.inner.ctx.journaled_state.database,
            &mut self.inner.inspector,
            &mut self.inner.precompiles,
        )
    }
}

#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct BerachainEvmFactory;

impl EvmFactory for BerachainEvmFactory {
    type Evm<DB: Database, I: Inspector<EthEvmContext<DB>>> =
        BerachainEvm<DB, I, Self::Precompiles>;
    type Context<DB: Database> = Context<BlockEnv, TxEnv, CfgEnv, DB>;
    type Tx = TxEnv;
    type Error<DBError: core::error::Error + Send + Sync + 'static> = EVMError<DBError>;
    type HaltReason = HaltReason;
    type Spec = SpecId;
    type Precompiles = PrecompilesMap;
    type BlockEnv = BlockEnv;

    fn create_evm<DB: Database>(&self, db: DB, input: EvmEnv) -> Self::Evm<DB, NoOpInspector> {
        BerachainEvmBuilder::new(db, input).build()
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: EvmEnv,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        BerachainEvmBuilder::new(db, input).activate_inspector(inspector).build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{U256, address};
    use reth::revm::{
        database_interface::EmptyDB, handler::SYSTEM_ADDRESS, primitives::hardfork::SpecId,
    };

    #[test]
    fn test_precompiles_with_correct_spec() {
        // create tests where precompile should be available for later specs but not earlier ones
        let specs_to_test = [
            // MODEXP (0x05) was added in Byzantium, should not exist in Frontier
            (
                address!("0x0000000000000000000000000000000000000005"),
                SpecId::FRONTIER,  // Early spec - should NOT have this precompile
                SpecId::BYZANTIUM, // Later spec - should have this precompile
                "MODEXP",
            ),
            // BLAKE2F (0x09) was added in Istanbul, should not exist in Byzantium
            (
                address!("0x0000000000000000000000000000000000000009"),
                SpecId::BYZANTIUM, // Early spec - should NOT have this precompile
                SpecId::ISTANBUL,  // Later spec - should have this precompile
                "BLAKE2F",
            ),
        ];

        for (precompile_addr, early_spec, later_spec, name) in specs_to_test {
            let mut early_cfg_env = CfgEnv::default();
            early_cfg_env.spec = early_spec;
            early_cfg_env.chain_id = 1;

            let early_env = EvmEnv { block_env: BlockEnv::default(), cfg_env: early_cfg_env };
            let factory = BerachainEvmFactory;
            let mut early_evm = factory.create_evm(EmptyDB::default(), early_env);

            // precompile should NOT be available in early spec
            assert!(
                early_evm.precompiles_mut().get(&precompile_addr).is_none(),
                "{name} precompile at {precompile_addr:?} should NOT be available for early spec {early_spec:?}"
            );

            let mut later_cfg_env = CfgEnv::default();
            later_cfg_env.spec = later_spec;
            later_cfg_env.chain_id = 1;

            let later_env = EvmEnv { block_env: BlockEnv::default(), cfg_env: later_cfg_env };
            let mut later_evm = factory.create_evm(EmptyDB::default(), later_env);

            // precompile should be available in later spec
            assert!(
                later_evm.precompiles_mut().get(&precompile_addr).is_some(),
                "{name} precompile at {precompile_addr:?} should be available for later spec {later_spec:?}"
            );
        }
    }

    #[test]
    fn test_pol_transaction_inspection() {
        // Tests that POL transactions work with call tracing
        // Fails if transact_system_call doesn't handle inspection properly

        use alloy_rpc_types_trace::geth::CallConfig;
        use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

        let evm_env = EvmEnv { cfg_env: CfgEnv::default(), block_env: BlockEnv::default() };
        let factory = BerachainEvmFactory;

        let call_config = CallConfig::default();
        let inspector_config = TracingInspectorConfig::from_geth_call_config(&call_config);
        let tracing_inspector = TracingInspector::new(inspector_config);

        let mut evm_with_inspector = factory.create_evm_with_inspector(
            EmptyDB::default(),
            evm_env.clone(),
            tracing_inspector,
        );

        let mut evm_no_inspector = factory.create_evm(EmptyDB::default(), evm_env);

        let recipient = address!("0x2000000000000000000000000000000000000002");
        let pol_tx = TxEnv {
            caller: SYSTEM_ADDRESS,
            gas_limit: 21000,
            gas_price: Default::default(),
            kind: TxKind::Call(recipient),
            value: U256::ONE,
            data: Bytes::new(),
            nonce: 0,
            chain_id: Some(1),
            access_list: Default::default(),
            gas_priority_fee: Default::default(),
            blob_hashes: vec![],
            max_fee_per_blob_gas: 0,
            authorization_list: vec![],
            tx_type: POL_TX_TYPE,
        };

        let result_with_tracer = evm_with_inspector.transact_raw(pol_tx.clone());
        let result_without_tracer = evm_no_inspector.transact_raw(pol_tx.clone());

        assert!(result_with_tracer.is_ok());
        assert!(result_without_tracer.is_ok());

        // Both should have gas_used = 0
        if let Ok(result) = &result_with_tracer &&
            let ExecutionResult::Success { gas_used, .. } = &result.result
        {
            assert_eq!(*gas_used, 0);
        }

        if let Ok(result) = &result_without_tracer &&
            let ExecutionResult::Success { gas_used, .. } = &result.result
        {
            assert_eq!(*gas_used, 0);
        }

        // Verify tracer captured system call details
        let (_, tracer, _) = evm_with_inspector.components_mut();
        let trace_result = tracer.clone().into_geth_builder().geth_call_traces(call_config, 0);

        assert_eq!(trace_result.from, pol_tx.caller);
        assert_eq!(trace_result.to, Some(recipient));
        assert!(!trace_result.calls.is_empty() || trace_result.gas > 0);
    }
}
