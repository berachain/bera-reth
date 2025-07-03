use crate::{
    chainspec::BerachainChainSpec, node::evm::assembler::BerachainAssembler,
    primitives::BerachainPrimitives,
};
use alloy_primitives::Bytes;
use reth_evm::{
    ConfigureEvm, EthEvmFactory, EvmEnvFor, ExecutionCtxFor, NextBlockEnvAttributes,
    eth::{EthBlockExecutorFactory, receipt_builder::AlloyReceiptBuilder, spec::EthSpec},
};
use reth_evm_ethereum::{EthBlockAssembler, EthEvmConfig, RethReceiptBuilder};
use reth_primitives_traits::{BlockTy, HeaderTy, SealedBlock, SealedHeader};
use std::{convert::Infallible, fmt::Debug, sync::Arc};

#[derive(Debug, Clone)]
pub struct BerachainEvmConfig<
    // TODO: ReceiptBuilder envelope
    R = AlloyReceiptBuilder,
    Spec = BerachainChainSpec,
    EvmFactory = EthEvmFactory,
> {
    /// Receipt builder.
    pub receipt_builder: R,
    /// Chain specification.
    pub spec: Spec,
    /// EVM factory.
    pub evm_factory: EvmFactory,
}

impl<ChainSpec, EvmFactory> BerachainEvmConfig<ChainSpec, EvmFactory> {
    /// Creates a new Ethereum EVM configuration with the given chain spec and EVM factory.
    pub fn new_with_evm_factory(chain_spec: Arc<ChainSpec>, evm_factory: EvmFactory) -> Self {
        todo!()
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
