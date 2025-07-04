use crate::{
    chainspec::BerachainChainSpec, node::evm::assembler::BerachainAssembler,
    primitives::BerachainPrimitives,
};
use alloy_primitives::Bytes;
use reth_evm::{
    ConfigureEvm, EthEvmFactory, EvmEnvFor, ExecutionCtxFor, NextBlockEnvAttributes,
    eth::EthBlockExecutorFactory,
};
use reth_evm_ethereum::{EthBlockAssembler, RethReceiptBuilder};
use reth_primitives_traits::{BlockTy, HeaderTy, SealedBlock, SealedHeader};
use std::{convert::Infallible, fmt::Debug, sync::Arc};

#[derive(Debug, Clone)]
pub struct BerachainEvmConfig {
    /// Receipt builder.
    pub receipt_builder: RethReceiptBuilder,
    /// Chain specification.
    pub spec: Arc<BerachainChainSpec>,
    /// EVM factory.
    pub evm_factory: EthEvmFactory,

    /// Inner [`EthBlockExecutorFactory`].
    pub executor_factory:
        EthBlockExecutorFactory<RethReceiptBuilder, Arc<BerachainChainSpec>, EthEvmFactory>,
    /// Ethereum block assembler.
    pub block_assembler: EthBlockAssembler<BerachainChainSpec>,
}

impl BerachainEvmConfig {
    /// Creates a new Ethereum EVM configuration with the given chain spec and EVM factory.
    pub fn new_with_evm_factory(
        chain_spec: Arc<BerachainChainSpec>,
        evm_factory: EthEvmFactory,
    ) -> Self {
        Self {
            receipt_builder: RethReceiptBuilder::default(),
            spec: chain_spec.clone(),
            block_assembler: EthBlockAssembler::new(chain_spec.clone()),
            executor_factory: EthBlockExecutorFactory::new(
                RethReceiptBuilder::default(),
                chain_spec,
                evm_factory,
            ),
            evm_factory,
        }
    }

    /// Sets the extra data for the block assembler.
    pub fn with_extra_data(mut self, extra_data: Bytes) -> Self {
        self.block_assembler.extra_data = extra_data;
        self
    }
}

impl ConfigureEvm for BerachainEvmConfig {
    type Primitives = BerachainPrimitives;
    type Error = Infallible;
    type NextBlockEnvCtx = NextBlockEnvAttributes;
    type BlockExecutorFactory = Self;
    type BlockAssembler = BerachainAssembler;

    fn block_executor_factory(&self) -> &Self::BlockExecutorFactory {
        todo!()
    }

    fn block_assembler(&self) -> &Self::BlockAssembler {
        todo!()
    }

    fn evm_env(&self, header: &HeaderTy<Self::Primitives>) -> EvmEnvFor<Self> {
        todo!()
    }

    fn next_evm_env(
        &self,
        parent: &HeaderTy<Self::Primitives>,
        attributes: &Self::NextBlockEnvCtx,
    ) -> Result<EvmEnvFor<Self>, Self::Error> {
        todo!()
    }

    fn context_for_block<'a>(
        &self,
        block: &'a SealedBlock<BlockTy<Self::Primitives>>,
    ) -> ExecutionCtxFor<'a, Self> {
        todo!()
    }

    fn context_for_next_block(
        &self,
        parent: &SealedHeader<HeaderTy<Self::Primitives>>,
        attributes: Self::NextBlockEnvCtx,
    ) -> ExecutionCtxFor<'_, Self> {
        todo!()
    }
}
