// use reth::revm::{
//     Context, Inspector, MainBuilder, MainContext,
//     context::{
//         BlockEnv, CfgEnv, TxEnv,
//         result::{EVMError, HaltReason},
//     },
//     inspector::NoOpInspector,
//     precompile::{PrecompileSpecId, Precompiles},
//     primitives::hardfork::SpecId,
// };
// use reth_evm::{Database, EthEvm, EvmEnv, EvmFactory, precompiles::PrecompilesMap};
// use std::error::Error;
//
// #[derive(Default, Debug, Clone, Copy)]
// pub struct BerachainEvmFactory;
//
// impl EvmFactory for BerachainEvmFactory {
//     type Evm<DB: Database, I: Inspector<Self::Context<DB>>> = EthEvm<DB, I, Self::Precompiles>;
//     type Context<DB: Database> = Context<BlockEnv, TxEnv, CfgEnv, DB>;
//     type Tx = TxEnv;
//     type Error<DBError: Error + Send + Sync + 'static> = EVMError<DBError>;
//     type HaltReason = HaltReason;
//     type Spec = SpecId;
//     type Precompiles = PrecompilesMap;
//
//     fn create_evm<DB: Database>(
//         &self,
//         db: DB,
//         input: EvmEnv<Self::Spec>,
//     ) -> Self::Evm<DB, NoOpInspector> {
//         todo!()
//     }
//
//     fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
//         &self,
//         db: DB,
//         input: EvmEnv<Self::Spec>,
//         inspector: I,
//     ) -> Self::Evm<DB, I> {
//         todo!()
//     }
// }
