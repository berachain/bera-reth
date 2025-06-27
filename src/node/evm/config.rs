use alloy_eips::eip4895::Withdrawals;
use alloy_primitives::{Address, B256, Bytes};
use reth_ethereum_forks::{EthBlockAssembler, EthBlockExecutorFactory};
use reth_evm::{ConfigureEvm, EthEvmFactory, NextBlockEnvAttributes, execute::EvmFactory};
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives::{EthPrimitives, Header, SealedBlock, SealedHeader};
use reth_revm::primitives::RethReceiptBuilder;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, sync::Arc};

use crate::{chainspec::BerachainChainSpec, payload::BlsPubkey};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BerachainNextBlockEnvAttributes {
    /// The timestamp of the next block.
    pub timestamp: u64,
    /// The suggested fee recipient for the next block.
    pub suggested_fee_recipient: Address,
    /// The randomness value for the next block.
    pub prev_randao: B256,
    /// Block gas limit.
    pub gas_limit: u64,
    /// The parent beacon block root.
    pub parent_beacon_block_root: Option<B256>,
    /// Withdrawals
    pub withdrawals: Option<Withdrawals>,
    /// Previous validator/proposer public key
    pub previous_proposer: BlsPubkey,
}

#[derive(Debug, Clone)]
pub struct BerachainEvmConfig {
    inner: EthEvmConfig<BerachainChainSpec>,
}

impl BerachainEvmConfig {
    pub fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { inner: EthEvmConfig::new_with_evm_factory(chain_spec, EthEvmFactory::default()) }
    }

    pub fn with_extra_data(mut self, extra_data: Bytes) -> Self {
        self.inner = self.inner.with_extra_data(extra_data);
        self
    }
}

impl ConfigureEvm for BerachainEvmConfig {
    type Primitives = EthPrimitives;
    type NextBlockEnvCtx = NextBlockEnvAttributes;
    type Error = Infallible;
    type BlockExecutorFactory =
        EthBlockExecutorFactory<RethReceiptBuilder, Arc<BerachainChainSpec>, EthEvmFactory>;
    type BlockAssembler = EthBlockAssembler<BerachainChainSpec>;

    fn block_executor_factory(&self) -> &Self::BlockExecutorFactory {
        self.inner.block_executor_factory()
    }

    fn block_assembler(&self) -> &Self::BlockAssembler {
        self.inner.block_assembler()
    }

    fn evm_env(&self, header: &Header) -> reth_evm::EvmEnv {
        self.inner.evm_env(header)
    }

    fn next_evm_env(
        &self,
        parent: &Header,
        attributes: &Self::NextBlockEnvCtx,
    ) -> Result<reth_evm::EvmEnv, Self::Error> {
        self.inner.next_evm_env(parent, attributes)
    }

    fn context_for_block<'a>(
        &self,
        block: &'a SealedBlock,
    ) -> <Self::BlockExecutorFactory as BlockExecutorFactory>::ExecutionCtx<'a> {
        self.inner.context_for_block(block)
    }

    fn context_for_next_block<'a>(
        &self,
        parent: &'a SealedHeader,
        attributes: Self::NextBlockEnvCtx,
    ) -> <Self::BlockExecutorFactory as BlockExecutorFactory>::ExecutionCtx<'a> {
        self.inner.context_for_next_block(parent, attributes)
    }
}

/*impl From<NextBlockEnvAttributes> for BerachainNextBlockEnvAttributes {
    fn from(attrs: NextBlockEnvAttributes) -> Self {
        Self {
            timestamp: attrs.timestamp,
            suggested_fee_recipient: attrs.suggested_fee_recipient,
            prev_randao: attrs.prev_randao,
            gas_limit: attrs.gas_limit,
            parent_beacon_block_root: attrs.parent_beacon_block_root,
            withdrawals: attrs.withdrawals,
            previous_proposer: BlsPubkey([0u8; 48]), // Default to zero
        }
    }
}*/

// #[derive(Debug, Clone)]
// pub struct BerachainEvmConfig {
//     chain_spec: BerachainChainSpec,
//     extra_data: Bytes,
// }

// impl BerachainEvmConfig {
//     pub fn new(chain_spec: BerachainChainSpec) -> Self {
//         Self { chain_spec, extra_data: Bytes::default() }
//     }
//
//     pub fn with_extra_data(mut self, extra_data: Bytes) -> Self {
//         self.extra_data = extra_data;
//         self
//     }
// }
//
// impl ConfigureEvm for BerachainEvmConfig {
//     type Primitives = reth_primitives::EthPrimitives;
//     type NextBlockEnvCtx = BerachainNextBlockEnvAttributes;
//
//     fn next_evm_env(
//         &self,
//         parent: &Header,
//         attributes: &Self::NextBlockEnvCtx,
//     ) -> Result<EvmEnv, reth_evm::ConfigureEvmError> {
//         let cfg_env = CfgEnv::default().with_chain_id(self.chain_spec.chain().id());
//
//         let block_env = BlockEnv {
//             number: reth_primitives::U256::from(parent.number + 1),
//             coinbase: attributes.suggested_fee_recipient,
//             timestamp: reth_primitives::U256::from(attributes.timestamp),
//             gas_limit: reth_primitives::U256::from(attributes.gas_limit),
//             basefee: reth_primitives::U256::from(reth_ethereum_forks::ForkCondition::<
//                 reth_primitives::BlockNumber,
//             >::next_block_base_fee(
//                 &self.chain_spec, parent, attributes.timestamp
//             )?),
//             difficulty: reth_primitives::U256::ZERO,
//             prevrandao: Some(attributes.prev_randao),
//             blob_excess_gas_and_price: None,
//         };
//
//         Ok(EvmEnv { cfg: cfg_env, block: block_env })
//     }
//
//     fn fill_tx_env<T>(&self, tx_env: &mut revm::TxEnv, transaction: T, sender: Address)
//     where
//         T: AsRef<TransactionSigned>,
//     {
//         reth_evm::ethereum::fill_tx_env(tx_env, transaction, sender)
//     }
//
//     fn fill_block_env(
//         &self,
//         block_env: &mut BlockEnv,
//         header: &Header,
//         after_merge: bool,
//     ) -> Result<(), reth_evm::ConfigureEvmError> {
//         reth_evm::ethereum::fill_block_env(block_env, header, after_merge, &self.extra_data)
//     }
//
//     fn fill_cfg_env(
//         &self,
//         cfg_env: &mut CfgEnv,
//         header: &Header,
//         total_difficulty: reth_primitives::U256,
//     ) {
//         reth_evm::ethereum::fill_cfg_env(&self.chain_spec, cfg_env, header, total_difficulty)
//     }
// }
//
// impl<ChainSpec> ConfigureEvmEnv for BerachainEvmConfig<ChainSpec>
// where
//     ChainSpec: EthChainSpec + EthereumHardforks,
// {
//     type Primitives = reth_primitives::EthPrimitives;
//     type Error = reth_evm::ConfigureEvmError;
//
//     fn fill_tx_env(
//         &self,
//         tx_env: &mut revm::TxEnv,
//         transaction: &TransactionSigned,
//         sender: Address,
//     ) {
//         ConfigureEvm::fill_tx_env(self, tx_env, transaction, sender)
//     }
//
//     fn fill_block_env(
//         &self,
//         block_env: &mut BlockEnv,
//         header: &Header,
//         after_merge: bool,
//     ) -> Result<(), Self::Error> {
//         ConfigureEvm::fill_block_env(self, block_env, header, after_merge)
//     }
//
//     fn fill_cfg_env(
//         &self,
//         cfg_env: &mut CfgEnv,
//         header: &Header,
//         total_difficulty: reth_primitives::U256,
//     ) {
//         ConfigureEvm::fill_cfg_env(self, cfg_env, header, total_difficulty)
//     }
// }
