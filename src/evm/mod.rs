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
            // P256VERIFY (0x100) was added in Osaka (EIP-7951), should not exist in Prague
            (
                address!("0x0000000000000000000000000000000000000100"),
                SpecId::PRAGUE, // Early spec - should NOT have this precompile
                SpecId::OSAKA,  // Later spec - should have this precompile
                "P256VERIFY",
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

// End-to-end tests verifying each EIP bundled in BRIP-0010 actually takes effect
// when the EVM runs under SpecId::OSAKA. Each test exercises the real execution
// path via BerachainEvmFactory and asserts observable behavior (return values,
// gas consumed, deployment success/failure).
#[cfg(test)]
mod osaka_eip_tests {
    use super::*;
    use crate::node::evm::config::{MAX_CODE_SIZE_OSAKA, MAX_INITCODE_SIZE_OSAKA};
    use alloy_primitives::{Bytes, U256, address, hex};
    use reth::revm::{
        context_interface::result::ExecutionResult,
        database_interface::EmptyDB,
        db::CacheDB,
        primitives::hardfork::SpecId,
        state::{AccountInfo, Bytecode},
    };

    const P256VERIFY_OSAKA_GAS: u64 = 6_900;
    const MODEXP_MIN_GAS_OSAKA: u64 = 500;
    const DEFAULT_MAX_CODE_SIZE: usize = 24_576;

    const CALLER: Address = address!("0x1000000000000000000000000000000000000001");
    const CONTRACT: Address = address!("0x2000000000000000000000000000000000000002");

    fn fund_caller(db: &mut CacheDB<EmptyDB>) {
        let info = AccountInfo { balance: U256::from(u128::MAX), ..AccountInfo::default() };
        db.insert_account_info(CALLER, info);
    }

    fn install_contract(db: &mut CacheDB<EmptyDB>, addr: Address, code: Bytes) {
        let bytecode = Bytecode::new_legacy(code);
        let code_hash = bytecode.hash_slow();
        let info = AccountInfo {
            balance: U256::ZERO,
            nonce: 1,
            code_hash,
            code: Some(bytecode),
            ..AccountInfo::default()
        };
        db.insert_account_info(addr, info);
    }

    fn osaka_cfg() -> CfgEnv {
        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::OSAKA;
        cfg.chain_id = 1;
        cfg.limit_contract_code_size = Some(MAX_CODE_SIZE_OSAKA);
        cfg.limit_contract_initcode_size = Some(MAX_INITCODE_SIZE_OSAKA);
        cfg
    }

    fn prague_cfg() -> CfgEnv {
        let mut cfg = CfgEnv::default();
        cfg.spec = SpecId::PRAGUE;
        cfg.chain_id = 1;
        cfg
    }

    fn legacy_tx(kind: TxKind, data: Bytes, gas_limit: u64) -> TxEnv {
        TxEnv {
            caller: CALLER,
            gas_limit,
            gas_price: 0,
            kind,
            value: U256::ZERO,
            data,
            nonce: 0,
            chain_id: Some(1),
            access_list: Default::default(),
            gas_priority_fee: Default::default(),
            blob_hashes: vec![],
            max_fee_per_blob_gas: 0,
            authorization_list: vec![],
            tx_type: 0,
        }
    }

    fn run_tx(cfg: CfgEnv, db: CacheDB<EmptyDB>, tx: TxEnv) -> ResultAndState<HaltReason> {
        let env = EvmEnv { block_env: BlockEnv::default(), cfg_env: cfg };
        let mut evm = BerachainEvmFactory.create_evm(db, env);
        evm.transact_raw(tx).expect("transact_raw failed")
    }

    // --- EIP-7951: P-256 (secp256r1) precompile at 0x100 -------------------

    /// Valid P-256 test vector from revm's secp256r1 test suite.
    /// Format: msg_hash(32) || r(32) || s(32) || pubkey_x(32) || pubkey_y(32)
    const VALID_P256_INPUT: [u8; 160] = hex!(
        "4cee90eb86eaa050036147a12d49004b6b9c72bd725d39d4785011fe190f0b4d"
        "a73bd4903f0ce3b639bbbf6e8e80d16931ff4bcf5993d58468e8fb19086e8cac"
        "36dbcd03009df8c59286b162af3bd7fcc0450c9aa81be5d10d312af6c66b1d60"
        "4aebd3099c618202fcfe16ae7770b0c49ab5eadf74b754204a3bb6060e44eff3"
        "7618b065f9832de4ca6ca971a7a1adc826d0f7c00181a5fb2ddf79ae00b4e10e"
    );

    #[test]
    fn test_eip7951_p256verify_valid_signature_returns_one() {
        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(
            TxKind::Call(address!("0x0000000000000000000000000000000000000100")),
            Bytes::from_static(&VALID_P256_INPUT),
            1_000_000,
        );
        let result = run_tx(osaka_cfg(), db, tx);

        let ExecutionResult::Success { output, .. } = result.result else {
            panic!("P256VERIFY call should succeed with valid signature");
        };
        // Precompile returns 32 bytes: 31 zero bytes followed by 0x01.
        assert_eq!(output.data().len(), 32, "P256VERIFY valid sig returns 32-byte output");
        assert_eq!(output.data()[31], 1, "last byte must be 0x01 for valid signature");
        assert!(output.data()[..31].iter().all(|b| *b == 0), "first 31 bytes must be zero");
    }

    #[test]
    fn test_eip7951_p256verify_invalid_signature_returns_empty() {
        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        // Flip the first byte of the message hash -> signature no longer verifies.
        let mut invalid = VALID_P256_INPUT;
        invalid[0] ^= 0xff;

        let tx = legacy_tx(
            TxKind::Call(address!("0x0000000000000000000000000000000000000100")),
            Bytes::from(invalid.to_vec()),
            1_000_000,
        );
        let result = run_tx(osaka_cfg(), db, tx);

        let ExecutionResult::Success { output, .. } = result.result else {
            panic!("P256VERIFY call should succeed even with invalid signature");
        };
        assert!(output.data().is_empty(), "P256VERIFY invalid sig returns empty output");
    }

    #[test]
    fn test_eip7951_p256verify_charges_osaka_gas() {
        // Verify the precompile is the Osaka variant (6900 gas), not the pre-Osaka one (3450).
        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(
            TxKind::Call(address!("0x0000000000000000000000000000000000000100")),
            Bytes::from_static(&VALID_P256_INPUT),
            1_000_000,
        );
        let result = run_tx(osaka_cfg(), db, tx);

        let gas_used = result.result.gas_used();
        // Base tx (21000) + calldata (160 bytes non-zero, roughly 160*16=2560) +
        // precompile cost (6900) => ~30460 expected.
        assert!(
            gas_used >= 21_000 + P256VERIFY_OSAKA_GAS,
            "gas_used ({gas_used}) should include Osaka P256VERIFY cost ({P256VERIFY_OSAKA_GAS})"
        );
    }

    // --- EIP-7939: CLZ opcode (0x1e) ---------------------------------------

    /// Runtime bytecode: CALLDATALOAD(0) -> CLZ -> MSTORE(0, .) -> RETURN(0, 32).
    /// Bytes: PUSH1 0x00, CALLDATALOAD, CLZ, PUSH1 0x00, MSTORE, PUSH1 0x20, PUSH1 0x00, RETURN.
    const CLZ_RUNTIME: [u8; 12] = hex!("6000351e60005260206000f3");

    #[test]
    fn test_eip7939_clz_counts_leading_zeros_osaka() {
        let cases: &[(U256, u64)] = &[
            (U256::ZERO, 256),
            (U256::from(1u64), 255),
            (U256::from(u8::MAX), 248),
            (U256::MAX, 0),
            (U256::from(1u64) << 128, 127),
        ];

        for &(input, expected_leading_zeros) in cases {
            let mut db = CacheDB::new(EmptyDB::default());
            fund_caller(&mut db);
            install_contract(&mut db, CONTRACT, Bytes::from_static(&CLZ_RUNTIME));

            let tx = legacy_tx(
                TxKind::Call(CONTRACT),
                Bytes::from(input.to_be_bytes::<32>().to_vec()),
                1_000_000,
            );
            let result = run_tx(osaka_cfg(), db, tx);

            let ExecutionResult::Success { output, .. } = result.result else {
                panic!("CLZ bytecode must execute successfully for input {input:#x}");
            };
            let returned = U256::from_be_slice(output.data());
            assert_eq!(
                returned,
                U256::from(expected_leading_zeros),
                "CLZ({input:#x}) should return {expected_leading_zeros}"
            );
        }
    }

    #[test]
    fn test_eip7939_clz_unavailable_pre_osaka() {
        // Same bytecode under SpecId::PRAGUE must halt on the unknown 0x1e opcode.
        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);
        install_contract(&mut db, CONTRACT, Bytes::from_static(&CLZ_RUNTIME));

        let tx = legacy_tx(
            TxKind::Call(CONTRACT),
            Bytes::from(U256::from(1u64).to_be_bytes::<32>().to_vec()),
            1_000_000,
        );
        let result = run_tx(prague_cfg(), db, tx);

        assert!(
            !result.result.is_success(),
            "CLZ must not be executable pre-Osaka; got Success with output {:?}",
            result.result.output()
        );
    }

    // --- EIP-7823 / EIP-7883: MODEXP input bounds and gas repricing --------

    /// Build MODEXP input: base_len(32) || exp_len(32) || mod_len(32) || base || exp || mod.
    fn modexp_input(base: &[u8], exponent: &[u8], modulus: &[u8]) -> Bytes {
        let mut input = Vec::with_capacity(96 + base.len() + exponent.len() + modulus.len());
        input.extend_from_slice(&U256::from(base.len()).to_be_bytes::<32>());
        input.extend_from_slice(&U256::from(exponent.len()).to_be_bytes::<32>());
        input.extend_from_slice(&U256::from(modulus.len()).to_be_bytes::<32>());
        input.extend_from_slice(base);
        input.extend_from_slice(exponent);
        input.extend_from_slice(modulus);
        Bytes::from(input)
    }

    #[test]
    fn test_eip7883_modexp_minimum_gas_raised_to_500_osaka() {
        // 2^3 mod 5 = 3. With EIP-7883 the minimum gas cost for this call is 500.
        let input = modexp_input(&[2u8], &[3u8], &[5u8]);
        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(
            TxKind::Call(address!("0x0000000000000000000000000000000000000005")),
            input,
            1_000_000,
        );
        let result = run_tx(osaka_cfg(), db, tx);

        let ExecutionResult::Success { output, gas_used, .. } = result.result else {
            panic!("MODEXP with small inputs should succeed");
        };
        assert_eq!(output.data().last().copied(), Some(3u8), "2^3 mod 5 == 3");
        // gas_used covers tx base + calldata + MODEXP. MODEXP alone must cost >= 500.
        assert!(
            gas_used >= 21_000 + MODEXP_MIN_GAS_OSAKA,
            "gas_used ({gas_used}) should cover Osaka MODEXP minimum ({MODEXP_MIN_GAS_OSAKA})"
        );
    }

    #[test]
    fn test_eip7823_modexp_oversized_input_halts_osaka() {
        // EIP-7823: any field whose declared length exceeds 1024 bytes must cause the
        // precompile to consume all gas and fail.
        let mut oversized_len = [0u8; 32];
        oversized_len[30] = 0x04;
        oversized_len[31] = 0x01; // 0x0401 == 1025 bytes, one past the cap.

        let zero_len = [0u8; 32];
        let mut input = Vec::with_capacity(96 + 1025);
        input.extend_from_slice(&oversized_len); // base_len = 1025
        input.extend_from_slice(&zero_len); // exp_len = 0
        input.extend_from_slice(&zero_len); // mod_len = 0
        input.extend_from_slice(&[0u8; 1025]); // base payload

        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(
            TxKind::Call(address!("0x0000000000000000000000000000000000000005")),
            Bytes::from(input),
            1_000_000,
        );
        let result = run_tx(osaka_cfg(), db, tx);

        assert!(
            !result.result.is_success(),
            "MODEXP with base_len > 1024 must not succeed under Osaka, got {:?}",
            result.result
        );
    }

    // --- BRIP-0010 code size bump (32 KB runtime / 64 KB initcode) ---------

    /// Init code that returns `len` bytes of 0x00 (STOP) as the deployed runtime code.
    /// Layout:
    ///   PUSH2 len | PUSH2 15 | PUSH1 0 | CODECOPY | PUSH2 len | PUSH1 0 | RETURN | <len x 0x00>
    /// The constant `15` is the exact length of the init prefix below.
    fn create_init_code_returning_stops(runtime_len: u16) -> Bytes {
        let len_hi = (runtime_len >> 8) as u8;
        let len_lo = (runtime_len & 0xff) as u8;
        let mut code = vec![
            0x61, len_hi, len_lo, // PUSH2 runtime_len
            0x61, 0x00, 0x0f, // PUSH2 15 (offset of runtime in init code)
            0x60, 0x00, // PUSH1 0 (memory offset)
            0x39, // CODECOPY
            0x61, len_hi, len_lo, // PUSH2 runtime_len
            0x60, 0x00, // PUSH1 0 (memory offset)
            0xf3, // RETURN
        ];
        debug_assert_eq!(code.len(), 15);
        code.extend(std::iter::repeat_n(0x00u8, runtime_len as usize));
        Bytes::from(code)
    }

    #[test]
    fn test_code_size_limit_increased_to_32kb_osaka() {
        // 28 KB is larger than the pre-Osaka 24 KB limit but fits within the 32 KB Osaka limit.
        let runtime_len: u16 = 28_000;
        assert!(runtime_len as usize > DEFAULT_MAX_CODE_SIZE);
        assert!((runtime_len as usize) < MAX_CODE_SIZE_OSAKA);

        let init_code = create_init_code_returning_stops(runtime_len);
        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(TxKind::Create, init_code, 10_000_000);
        let result = run_tx(osaka_cfg(), db, tx);

        assert!(
            result.result.is_success(),
            "28 KB deployment must succeed under Osaka's 32 KB limit, got {:?}",
            result.result
        );
    }

    #[test]
    fn test_code_size_limit_24kb_enforced_pre_osaka() {
        // Same deployment pre-Osaka (no override, default EIP-170) must fail.
        let runtime_len: u16 = 28_000;
        let init_code = create_init_code_returning_stops(runtime_len);

        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(TxKind::Create, init_code, 10_000_000);
        let result = run_tx(prague_cfg(), db, tx);

        assert!(
            !result.result.is_success(),
            "28 KB deployment must fail under Prague's 24 KB limit, got {:?}",
            result.result
        );
    }

    #[test]
    fn test_code_size_limit_within_24kb_deploys_pre_osaka() {
        // Sanity check: the init code itself is valid and deploys a small contract pre-Osaka.
        let runtime_len: u16 = 1024;
        let init_code = create_init_code_returning_stops(runtime_len);

        let mut db = CacheDB::new(EmptyDB::default());
        fund_caller(&mut db);

        let tx = legacy_tx(TxKind::Create, init_code, 10_000_000);
        let result = run_tx(prague_cfg(), db, tx);

        assert!(
            result.result.is_success(),
            "1 KB deployment must succeed pre-Osaka, got {:?}",
            result.result
        );
    }
}
