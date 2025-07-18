use crate::{
    chainspec::BerachainChainSpec,
    node::evm::{
        assembler::BerachainBlockAssembler, block_context::BerachainBlockExecutionCtx,
        receipt::BerachainReceiptBuilder,
    },
    primitives::{BerachainHeader, BerachainPrimitives, header::BlsPublicKey},
};
use alloy_consensus::BlockHeader;
use alloy_eips::{eip1559::INITIAL_BASE_FEE, eip4895::Withdrawals, eip7840::BlobParams};
use alloy_primitives::{Address, B256, Bytes, U256};
use reth::{
    chainspec::{EthereumHardfork, Hardforks},
    revm::{
        context::{BlockEnv, CfgEnv},
        context_interface::block::BlobExcessGasAndPrice,
        primitives::hardfork::SpecId,
    },
};
use reth_chainspec::EthChainSpec;
use reth_evm::{ConfigureEvm, EthEvmFactory, EvmEnv, EvmEnvFor, ExecutionCtxFor};
use reth_evm_ethereum::{revm_spec, revm_spec_by_timestamp_and_block_number};
use reth_primitives_traits::{BlockTy, HeaderTy, SealedBlock, SealedHeader};
use reth_rpc_eth_api::helpers::pending_block::BuildPendingEnv;
use std::{borrow::Cow, convert::Infallible, fmt::Debug, sync::Arc};

#[derive(Debug, Clone)]
pub struct BerachainEvmConfig {
    /// Receipt builder.
    pub receipt_builder: BerachainReceiptBuilder,
    /// Chain specification.
    pub spec: Arc<BerachainChainSpec>,
    /// EVM factory.
    pub evm_factory: EthEvmFactory,

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
            receipt_builder: BerachainReceiptBuilder::default(),
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

    pub fn chain_spec(&self) -> &BerachainChainSpec {
        &self.spec
    }
}

/// Attributes for the next block environment for Berachain.
#[derive(Debug, Clone)]
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
    /// Previous proposer public key.
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
}

impl ConfigureEvm for BerachainEvmConfig {
    type Primitives = BerachainPrimitives;
    type Error = Infallible;

    type NextBlockEnvCtx = BerachainNextBlockEnvAttributes;
    type BlockExecutorFactory = Self;
    type BlockAssembler = BerachainBlockAssembler;

    fn block_executor_factory(&self) -> &Self::BlockExecutorFactory {
        self
    }

    fn block_assembler(&self) -> &Self::BlockAssembler {
        &self.block_assembler
    }

    fn evm_env(&self, header: &HeaderTy<Self::Primitives>) -> EvmEnvFor<Self> {
        let blob_params = self.chain_spec().blob_params_at_timestamp(header.timestamp);
        let spec = revm_spec::<BerachainChainSpec, BerachainHeader>(self.chain_spec(), header);

        // configure evm env based on parent block
        let mut cfg_env =
            CfgEnv::new().with_chain_id(self.chain_spec().chain().id()).with_spec(spec);

        if let Some(blob_params) = &blob_params {
            cfg_env.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        // derive the EIP-4844 blob fees from the header's `excess_blob_gas` and the current
        // blobparams
        let blob_excess_gas_and_price =
            header.excess_blob_gas.zip(blob_params).map(|(excess_blob_gas, params)| {
                let blob_gasprice = params.calc_blob_fee(excess_blob_gas);
                BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
            });

        let block_env = BlockEnv {
            number: U256::from(header.number()),
            beneficiary: header.beneficiary(),
            timestamp: U256::from(header.timestamp()),
            difficulty: if spec >= SpecId::MERGE { U256::ZERO } else { header.difficulty() },
            prevrandao: if spec >= SpecId::MERGE { header.mix_hash() } else { None },
            gas_limit: header.gas_limit(),
            basefee: header.base_fee_per_gas().unwrap_or_default(),
            blob_excess_gas_and_price,
        };

        EvmEnv { cfg_env, block_env }
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
        BerachainBlockExecutionCtx {
            parent_hash: block.header().parent_hash,
            parent_beacon_block_root: block.header().parent_beacon_block_root,
            ommers: &block.body().ommers,
            withdrawals: block.body().withdrawals.as_ref().map(Cow::Borrowed),
            prev_proposer_pubkey: block.header().prev_proposer_pubkey,
        }
    }

    fn context_for_next_block(
        &self,
        parent: &SealedHeader<HeaderTy<Self::Primitives>>,
        attributes: Self::NextBlockEnvCtx,
    ) -> ExecutionCtxFor<'_, Self> {
        BerachainBlockExecutionCtx {
            parent_hash: parent.hash(),
            parent_beacon_block_root: attributes.parent_beacon_block_root,
            ommers: &[],
            withdrawals: attributes.withdrawals.map(Cow::Owned),
            prev_proposer_pubkey: attributes.prev_proposer_pubkey,
        }
    }
}

impl BuildPendingEnv<BerachainHeader> for BerachainNextBlockEnvAttributes {
    fn build_pending_env(parent: &SealedHeader<BerachainHeader>) -> Self {
        Self {
            timestamp: parent.timestamp().saturating_add(2),
            suggested_fee_recipient: parent.beneficiary(),
            prev_randao: B256::random(),
            gas_limit: parent.gas_limit(),
            parent_beacon_block_root: parent.parent_beacon_block_root().map(|_| B256::ZERO),
            withdrawals: parent.withdrawals_root().map(|_| Default::default()),
            prev_proposer_pubkey: parent.header().prev_proposer_pubkey,
        }
    }
}
