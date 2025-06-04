//
// #![cfg_attr(not(test), warn(unused_crate_dependencies))]
// #![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
// #![cfg_attr(not(feature = "std"), no_std)]
//
// use std::borrow::Cow;
// use std::convert::Infallible;
// use std::fmt::Debug;
// use std::sync::Arc;
// use alloy_consensus::{BlockHeader, Header};
// use alloy_eips::eip7840::BlobParams;
// use reth::core::primitives::SealedBlock;
// use reth::primitives::{Block, EthPrimitives, SealedHeader, TransactionSigned};
// use reth::revm::context::{BlockEnv, CfgEnv};
// use reth::revm::context_interface::block::BlobExcessGasAndPrice;
// use reth::revm::primitives::{Bytes, U256};
// use reth::revm::primitives::hardfork::SpecId;
// use reth_chainspec::EthChainSpec;
// use reth_evm::eth::{EthBlockExecutionCtx, EthBlockExecutorFactory};
// use reth_evm::{ConfigureEvm, EthEvmFactory, EvmEnv, EvmFactory, FromRecoveredTx, FromTxWithEncoded, NextBlockEnvAttributes, TransactionEnv};
// use reth_evm::precompiles::PrecompilesMap;
// use reth_evm_ethereum::{revm_spec, revm_spec_by_timestamp_and_block_number, EthBlockAssembler, RethReceiptBuilder};
// use crate::chainspec::BerachainChainSpec;
//
// /// Ethereum-related EVM configuration.
// #[derive(Debug, Clone)]
// pub struct BeraEvmConfig<EvmFactory = EthEvmFactory> {
//     /// Inner [`EthBlockExecutorFactory`].
//     pub executor_factory: EthBlockExecutorFactory<RethReceiptBuilder, Arc<BerachainChainSpec>, EvmFactory>,
//     /// Ethereum block assembler.
//     pub block_assembler: EthBlockAssembler<BerachainChainSpec>,
// }
//
// impl BeraEvmConfig {
//     /// Creates a new Ethereum EVM configuration with the given chain spec.
//     pub fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
//         Self::berachain(chain_spec)
//     }
//
//     /// Creates a new Ethereum EVM configuration.
//     pub fn berachain(chain_spec: Arc<BerachainChainSpec>) -> Self {
//         Self::new_with_evm_factory(chain_spec, EthEvmFactory::default())
//     }
// }
//
// impl<EvmFactory> BeraEvmConfig<EvmFactory> {
//     /// Creates a new Ethereum EVM configuration with the given chain spec and EVM factory.
//     pub fn new_with_evm_factory(chain_spec: Arc<BerachainChainSpec>, evm_factory: EvmFactory) -> Self {
//         Self {
//             block_assembler: EthBlockAssembler::new(chain_spec.clone()),
//             executor_factory: EthBlockExecutorFactory::new(
//                 RethReceiptBuilder::default(),
//                 chain_spec,
//                 evm_factory,
//             ),
//         }
//     }
//
//     /// Returns the chain spec associated with this configuration.
//     pub const fn chain_spec(&self) -> &Arc<BerachainChainSpec> {
//         self.executor_factory.spec()
//     }
//
//     /// Sets the extra data for the block assembler.
//     pub fn with_extra_data(mut self, extra_data: Bytes) -> Self {
//         self.block_assembler.extra_data = extra_data;
//         self
//     }
// }
//
// impl<EvmF> ConfigureEvm for BeraEvmConfig<EvmF>
// where
//     EvmF: EvmFactory<
//         Tx: TransactionEnv
//         + FromRecoveredTx<TransactionSigned>
//         + FromTxWithEncoded<TransactionSigned>,
//         Spec = SpecId,
//         Precompiles = PrecompilesMap,
//     > + Clone
//     + Debug
//     + Send
//     + Sync
//     + Unpin
//     + 'static,
// {
//     type Primitives = EthPrimitives;
//     type Error = Infallible;
//     type NextBlockEnvCtx = NextBlockEnvAttributes;
//     type BlockExecutorFactory = EthBlockExecutorFactory<RethReceiptBuilder, Arc<BerachainChainSpec>, EvmF>;
//     type BlockAssembler = EthBlockAssembler<BerachainChainSpec>;
//
//     fn block_executor_factory(&self) -> &Self::BlockExecutorFactory {
//         &self.executor_factory
//     }
//
//     fn block_assembler(&self) -> &Self::BlockAssembler {
//         &self.block_assembler
//     }
//
//     fn evm_env(&self, header: &Header) -> EvmEnv {
//         let blob_params = self.chain_spec().blob_params_at_timestamp(header.timestamp);
//         let spec = revm_spec(&self.chain_spec().inner(), header);
//
//         // configure evm env based on parent block
//         let mut cfg_env =
//             CfgEnv::new().with_chain_id(self.chain_spec().chain().id()).with_spec(spec);
//
//         if let Some(blob_params) = &blob_params {
//             cfg_env.set_blob_max_count(blob_params.max_blob_count);
//         }
//
//         // derive the EIP-4844 blob fees from the header's `excess_blob_gas` and the current
//         // blobparams
//         let blob_excess_gas_and_price =
//             header.excess_blob_gas.zip(blob_params).map(|(excess_blob_gas, params)| {
//                 let blob_gasprice = params.calc_blob_fee(excess_blob_gas);
//                 BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
//             });
//
//         let block_env = BlockEnv {
//             number: header.number(),
//             beneficiary: header.beneficiary(),
//             timestamp: header.timestamp(),
//             difficulty: if spec >= SpecId::MERGE { U256::ZERO } else { header.difficulty() },
//             prevrandao: if spec >= SpecId::MERGE { header.mix_hash() } else { None },
//             gas_limit: header.gas_limit(),
//             basefee: header.base_fee_per_gas().unwrap_or_default(),
//             blob_excess_gas_and_price,
//         };
//
//         EvmEnv { cfg_env, block_env }
//     }
//
//     fn next_evm_env(
//         &self,
//         parent: &Header,
//         attributes: &NextBlockEnvAttributes,
//     ) -> Result<EvmEnv, Self::Error> {
//         // ensure we're not missing any timestamp based hardforks
//         let chain_spec = self.chain_spec();
//         let blob_params = chain_spec.blob_params_at_timestamp(attributes.timestamp);
//         let spec_id = revm_spec_by_timestamp_and_block_number(
//             &self.chain_spec().inner(),
//             attributes.timestamp,
//             parent.number() + 1,
//         );
//
//         // configure evm env based on parent block
//         let mut cfg =
//             CfgEnv::new().with_chain_id(self.chain_spec().chain().id()).with_spec(spec_id);
//
//         if let Some(blob_params) = &blob_params {
//             cfg.set_blob_max_count(blob_params.max_blob_count);
//         }
//
//         // if the parent block did not have excess blob gas (i.e. it was pre-cancun), but it is
//         // cancun now, we need to set the excess blob gas to the default value(0)
//         let blob_excess_gas_and_price = parent
//             .maybe_next_block_excess_blob_gas(blob_params)
//             .or_else(|| (spec_id == SpecId::CANCUN).then_some(0))
//             .map(|excess_blob_gas| {
//                 let blob_gasprice =
//                     blob_params.unwrap_or_else(BlobParams::cancun).calc_blob_fee(excess_blob_gas);
//                 BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
//             });
//
//         let mut basefee = parent.next_block_base_fee(
//             self.chain_spec().base_fee_params_at_timestamp(attributes.timestamp),
//         );
//
//         let mut gas_limit = attributes.gas_limit;
//
//         let block_env = BlockEnv {
//             number: parent.number + 1,
//             beneficiary: attributes.suggested_fee_recipient,
//             timestamp: attributes.timestamp,
//             difficulty: U256::ZERO,
//             prevrandao: Some(attributes.prev_randao),
//             gas_limit,
//             // calculate basefee based on parent block's gas usage
//             basefee: basefee.unwrap_or_default(),
//             // calculate excess gas based on parent block's blob gas usage
//             blob_excess_gas_and_price,
//         };
//
//         Ok((cfg, block_env).into())
//     }
//
//     fn context_for_block<'a>(&self, block: &'a SealedBlock<Block>) -> EthBlockExecutionCtx<'a> {
//         EthBlockExecutionCtx {
//             parent_hash: block.header().parent_hash,
//             parent_beacon_block_root: block.header().parent_beacon_block_root,
//             ommers: &block.body().ommers,
//             withdrawals: block.body().withdrawals.as_ref().map(Cow::Borrowed),
//         }
//     }
//
//     fn context_for_next_block(
//         &self,
//         parent: &SealedHeader,
//         attributes: Self::NextBlockEnvCtx,
//     ) -> EthBlockExecutionCtx<'_> {
//         EthBlockExecutionCtx {
//             parent_hash: parent.hash(),
//             parent_beacon_block_root: attributes.parent_beacon_block_root,
//             ommers: &[],
//             withdrawals: attributes.withdrawals.map(Cow::Owned),
//         }
//     }
// }