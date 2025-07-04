use crate::{
    chainspec::BerachainChainSpec, node::evm::assembler::BerachainBlockAssembler,
    primitives::BerachainPrimitives,
};
use alloy_consensus::BlockHeader;
use alloy_eips::{eip1559::INITIAL_BASE_FEE, eip7840::BlobParams};
use alloy_primitives::{Bytes, U256};
use reth::{
    chainspec::{EthereumHardfork, Hardforks},
    revm::{
        context::{BlockEnv, CfgEnv},
        context_interface::block::BlobExcessGasAndPrice,
        primitives::hardfork::SpecId,
    },
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_evm::{
    ConfigureEvm, EthEvmFactory, EvmEnvFor, ExecutionCtxFor, NextBlockEnvAttributes,
    eth::EthBlockExecutorFactory,
};
use reth_evm_ethereum::{
    EthBlockAssembler, RethReceiptBuilder, revm_spec_by_timestamp_and_block_number,
};
use reth_primitives_traits::{BlockTy, HeaderTy, SealedBlock, SealedHeader};
use std::{convert::Infallible, fmt::Debug, sync::Arc};

#[derive(Debug, Clone)]
pub struct BerachainEvmConfig {
    /// Receipt builder.
    pub receipt_builder: RethReceiptBuilder,
    /// Chain specification.
    pub spec: Arc<BerachainChainSpec>,
    /// EVM factory.
    evm_factory: EthEvmFactory,

    /// Ethereum block assembler.
    pub block_assembler: BerachainBlockAssembler,
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
            block_assembler: BerachainBlockAssembler::new(chain_spec.clone()),
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
    type BlockAssembler = BerachainBlockAssembler;

    fn block_executor_factory(&self) -> &Self::BlockExecutorFactory {
        self
    }

    fn block_assembler(&self) -> &Self::BlockAssembler {
        &self.block_assembler
    }

    fn evm_env(&self, header: &HeaderTy<Self::Primitives>) -> EvmEnvFor<Self> {
        todo!()
    }

    fn next_evm_env(
        &self,
        parent: &HeaderTy<Self::Primitives>,
        attributes: &Self::NextBlockEnvCtx,
    ) -> Result<EvmEnvFor<Self>, Self::Error> {
        // ensure we're not missing any timestamp based hardforks
        let chain_spec = self.spec.as_ref();
        let blob_params = chain_spec.blob_params_at_timestamp(attributes.timestamp);
        let spec_id = revm_spec_by_timestamp_and_block_number(
            chain_spec,
            attributes.timestamp,
            parent.number() + 1,
        );
        // configure evm env based on parent block
        let mut cfg = CfgEnv::new().with_chain_id(chain_spec.chain().id()).with_spec(spec_id);

        if let Some(blob_params) = &blob_params {
            cfg.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        // if the parent block did not have excess blob gas (i.e. it was pre-cancun), but it is
        // cancun now, we need to set the excess blob gas to the default value(0)
        let blob_excess_gas_and_price = parent
            .maybe_next_block_excess_blob_gas(blob_params)
            .or_else(|| (spec_id == SpecId::CANCUN).then_some(0))
            .map(|excess_blob_gas| {
                let blob_gasprice =
                    blob_params.unwrap_or_else(BlobParams::cancun).calc_blob_fee(excess_blob_gas);
                BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
            });

        let mut basefee = chain_spec.next_block_base_fee(parent, attributes.timestamp);

        let mut gas_limit = attributes.gas_limit;

        // If we are on the London fork boundary, we need to multiply the parent's gas limit by the
        // elasticity multiplier to get the new gas limit.
        if chain_spec.fork(EthereumHardfork::London).transitions_at_block(parent.number + 1) {
            let elasticity_multiplier =
                chain_spec.base_fee_params_at_timestamp(attributes.timestamp).elasticity_multiplier;

            // multiply the gas limit by the elasticity multiplier
            gas_limit *= elasticity_multiplier as u64;

            // set the base fee to the initial base fee from the EIP-1559 spec
            basefee = Some(INITIAL_BASE_FEE)
        }

        let block_env = BlockEnv {
            number: U256::from(parent.number + 1),
            beneficiary: attributes.suggested_fee_recipient,
            timestamp: U256::from(attributes.timestamp),
            difficulty: U256::ZERO,
            prevrandao: Some(attributes.prev_randao),
            gas_limit,
            // calculate basefee based on parent block's gas usage
            basefee: basefee.unwrap_or_default(),
            // calculate excess gas based on parent block's blob gas usage
            blob_excess_gas_and_price,
        };

        Ok((cfg, block_env).into())
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
