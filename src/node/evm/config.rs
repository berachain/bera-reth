use crate::{
    chainspec::BerachainChainSpec,
    engine::BerachainExecutionData,
    evm::BerachainEvmFactory,
    node::evm::{
        assembler::BerachainBlockAssembler, block_context::BerachainBlockExecutionCtx,
        builder::BerachainBlockBuilder, receipt::BerachainReceiptBuilder,
    },
    primitives::{BerachainHeader, BerachainPrimitives, header::BlsPublicKey},
};
use alloy_consensus::BlockHeader;
use alloy_eips::{
    Decodable2718, eip1559::INITIAL_BASE_FEE, eip4895::Withdrawals, eip7840::BlobParams,
};
use alloy_primitives::{Address, B256, Bytes, U256};
use reth::{
    chainspec::{EthereumHardfork, EthereumHardforks, Hardforks},
    providers::errors::any::AnyError,
    revm::{
        State,
        context::{BlockEnv, CfgEnv},
        context_interface::block::BlobExcessGasAndPrice,
        primitives::hardfork::SpecId,
    },
};
use reth_chainspec::EthChainSpec;
use reth_engine_primitives::ExecutionPayload;
use reth_evm::{
    ConfigureEngineEvm, ConfigureEvm, Database, EvmEnv, EvmEnvFor, EvmFor, ExecutableTxIterator,
    ExecutionCtxFor, InspectorFor, block::BlockExecutorFactory, execute::BasicBlockBuilder,
};
use reth_evm_ethereum::{revm_spec, revm_spec_by_timestamp_and_block_number};
use reth_primitives_traits::{
    BlockTy, HeaderTy, SealedBlock, SealedHeader, SignedTransaction, TxTy,
    constants::MAX_TX_GAS_LIMIT_OSAKA,
};
use reth_rpc_eth_api::helpers::pending_block::BuildPendingEnv;
use std::{borrow::Cow, convert::Infallible, fmt::Debug, sync::Arc};

const BERACHAIN_BLOCK_TIME_SECONDS: u64 = 2;

/// BRIP-0010: Contract code size limit increase from 24 KB to 32 KB (Osaka)
pub(crate) const MAX_CODE_SIZE_OSAKA: usize = 32_768;

/// BRIP-0010: Contract initcode size limit increase from 48 KB to 64 KB (Osaka)
pub(crate) const MAX_INITCODE_SIZE_OSAKA: usize = 65_536;

#[derive(Debug, Clone)]
pub struct BerachainEvmConfig {
    /// Receipt builder.
    pub receipt_builder: BerachainReceiptBuilder,
    /// Chain specification.
    pub spec: Arc<BerachainChainSpec>,
    /// EVM factory.
    pub evm_factory: BerachainEvmFactory,

    /// Ethereum block assembler.
    pub block_assembler: BerachainBlockAssembler,
}

impl BerachainEvmConfig {
    /// Creates a new Ethereum EVM configuration with the given chain spec and EVM factory.
    pub fn new_with_evm_factory(
        chain_spec: Arc<BerachainChainSpec>,
        evm_factory: BerachainEvmFactory,
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
    /// Extra data for the block.
    pub extra_data: Bytes,
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

    fn evm_env(&self, header: &HeaderTy<Self::Primitives>) -> Result<EvmEnvFor<Self>, Self::Error> {
        let blob_params = self.chain_spec().blob_params_at_timestamp(header.timestamp);
        let spec = revm_spec::<BerachainChainSpec, BerachainHeader>(self.chain_spec(), header);

        // configure evm env based on parent block
        let mut cfg_env = CfgEnv::new()
            .with_chain_id(self.chain_spec().chain().id())
            .with_spec_and_mainnet_gas_params(spec);

        if let Some(blob_params) = &blob_params {
            cfg_env.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        if self.chain_spec().is_osaka_active_at_timestamp(header.timestamp) {
            cfg_env.tx_gas_limit_cap = Some(MAX_TX_GAS_LIMIT_OSAKA);
            cfg_env.limit_contract_code_size = Some(MAX_CODE_SIZE_OSAKA);
            cfg_env.limit_contract_initcode_size = Some(MAX_INITCODE_SIZE_OSAKA);
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
            slot_num: 0,
        };

        Ok(EvmEnv { cfg_env, block_env })
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
        let mut cfg = CfgEnv::new()
            .with_chain_id(chain_spec.chain().id())
            .with_spec_and_mainnet_gas_params(spec_id);

        if let Some(blob_params) = &blob_params {
            cfg.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        if self.chain_spec().is_osaka_active_at_timestamp(attributes.timestamp) {
            cfg.tx_gas_limit_cap = Some(MAX_TX_GAS_LIMIT_OSAKA);
            cfg.limit_contract_code_size = Some(MAX_CODE_SIZE_OSAKA);
            cfg.limit_contract_initcode_size = Some(MAX_INITCODE_SIZE_OSAKA);
        }

        // if the parent block did not have excess blob gas (i.e. it was pre-cancun), but it is
        // cancun now, we need to set the excess blob gas to the default value(0)
        let blob_excess_gas_and_price = parent
            .maybe_next_block_excess_blob_gas(blob_params)
            .or_else(|| blob_params.map(|_| 0))
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
            slot_num: 0,
        };

        Ok((cfg, block_env).into())
    }

    fn context_for_block<'a>(
        &self,
        block: &'a SealedBlock<BlockTy<Self::Primitives>>,
    ) -> Result<ExecutionCtxFor<'a, Self>, Self::Error> {
        Ok(BerachainBlockExecutionCtx {
            parent_hash: block.header().parent_hash,
            parent_beacon_block_root: block.header().parent_beacon_block_root,
            ommers: &block.body().ommers,
            withdrawals: block.body().withdrawals.as_ref().map(Cow::Borrowed),
            prev_proposer_pubkey: block.header().prev_proposer_pubkey,
        })
    }

    fn context_for_next_block(
        &self,
        parent: &SealedHeader<HeaderTy<Self::Primitives>>,
        attributes: Self::NextBlockEnvCtx,
    ) -> Result<ExecutionCtxFor<'_, Self>, Self::Error> {
        Ok(BerachainBlockExecutionCtx {
            parent_hash: parent.hash(),
            parent_beacon_block_root: attributes.parent_beacon_block_root,
            ommers: &[],
            withdrawals: attributes.withdrawals.map(Cow::Owned),
            prev_proposer_pubkey: attributes.prev_proposer_pubkey,
        })
    }

    fn create_block_builder<'a, DB, I>(
        &'a self,
        evm: EvmFor<Self, &'a mut State<DB>, I>,
        parent: &'a SealedHeader<HeaderTy<Self::Primitives>>,
        ctx: <Self::BlockExecutorFactory as BlockExecutorFactory>::ExecutionCtx<'a>,
    ) -> impl reth_evm::execute::BlockBuilder<
        Primitives = Self::Primitives,
        Executor = reth_evm::BlockExecutorForEvm<'a, Self, DB, I>,
    >
    where
        DB: Database,
        I: InspectorFor<Self, &'a mut State<DB>> + 'a,
    {
        let inner: BasicBlockBuilder<'a, Self, _, BerachainBlockAssembler, BerachainPrimitives> =
            BasicBlockBuilder {
                executor: BlockExecutorFactory::create_executor(self, evm, ctx.clone()),
                ctx,
                assembler: self.block_assembler().clone(),
                parent,
                transactions: Vec::new(),
            };
        BerachainBlockBuilder::new(inner, self.spec.clone())
    }
}

impl BuildPendingEnv<BerachainHeader> for BerachainNextBlockEnvAttributes {
    fn build_pending_env(
        parent: &SealedHeader<BerachainHeader>,
        _block_overrides: Option<&alloy_rpc_types_eth::BlockOverrides>,
    ) -> Self {
        Self {
            timestamp: parent.timestamp().saturating_add(BERACHAIN_BLOCK_TIME_SECONDS),
            suggested_fee_recipient: parent.beneficiary(),
            prev_randao: B256::random(),
            gas_limit: parent.gas_limit(),
            parent_beacon_block_root: parent.parent_beacon_block_root().map(|_| B256::ZERO),
            withdrawals: parent.withdrawals_root().map(|_| Default::default()),
            prev_proposer_pubkey: parent.header().prev_proposer_pubkey,
            extra_data: Default::default(),
        }
    }
}

impl ConfigureEngineEvm<BerachainExecutionData> for BerachainEvmConfig {
    fn evm_env_for_payload(
        &self,
        payload: &BerachainExecutionData,
    ) -> Result<EvmEnvFor<Self>, core::convert::Infallible> {
        let timestamp = payload.payload.timestamp();
        let block_number = payload.payload.block_number();

        let blob_params = self.chain_spec().blob_params_at_timestamp(timestamp);
        let spec =
            revm_spec_by_timestamp_and_block_number(self.chain_spec(), timestamp, block_number);

        // configure evm env based on parent block
        let mut cfg_env = CfgEnv::new()
            .with_chain_id(self.chain_spec().chain().id())
            .with_spec_and_mainnet_gas_params(spec);

        if let Some(blob_params) = &blob_params {
            cfg_env.set_max_blobs_per_tx(blob_params.max_blobs_per_tx);
        }

        if self.chain_spec().is_osaka_active_at_timestamp(timestamp) {
            cfg_env.tx_gas_limit_cap = Some(MAX_TX_GAS_LIMIT_OSAKA);
            cfg_env.limit_contract_code_size = Some(MAX_CODE_SIZE_OSAKA);
            cfg_env.limit_contract_initcode_size = Some(MAX_INITCODE_SIZE_OSAKA);
        }

        // derive the EIP-4844 blob fees from the header's `excess_blob_gas` and the current
        // blobparams
        let blob_excess_gas_and_price =
            payload.payload.excess_blob_gas().zip(blob_params).map(|(excess_blob_gas, params)| {
                let blob_gasprice = params.calc_blob_fee(excess_blob_gas);
                BlobExcessGasAndPrice { excess_blob_gas, blob_gasprice }
            });

        let block_env = BlockEnv {
            number: U256::from(block_number),
            beneficiary: payload.payload.fee_recipient(),
            timestamp: U256::from(timestamp),
            difficulty: if spec >= SpecId::MERGE {
                U256::ZERO
            } else {
                payload.payload.as_v1().prev_randao.into()
            },
            prevrandao: (spec >= SpecId::MERGE).then(|| payload.payload.as_v1().prev_randao),
            gas_limit: payload.payload.gas_limit(),
            basefee: payload.payload.saturated_base_fee_per_gas(),
            blob_excess_gas_and_price,
            slot_num: 0,
        };

        Ok(EvmEnv { cfg_env, block_env })
    }

    fn context_for_payload<'a>(
        &self,
        payload: &'a BerachainExecutionData,
    ) -> Result<ExecutionCtxFor<'a, Self>, core::convert::Infallible> {
        Ok(BerachainBlockExecutionCtx {
            parent_hash: payload.parent_hash(),
            parent_beacon_block_root: payload.sidecar.parent_beacon_block_root(),
            ommers: &[],
            withdrawals: payload.payload.withdrawals().map(|w| Cow::Owned(w.clone().into())),
            prev_proposer_pubkey: payload.sidecar.parent_proposer_pub_key,
        })
    }

    fn tx_iterator_for_payload(
        &self,
        payload: &BerachainExecutionData,
    ) -> Result<impl ExecutableTxIterator<Self>, core::convert::Infallible> {
        Ok((payload.payload.transactions().clone(), |tx: Bytes| {
            let tx =
                TxTy::<Self::Primitives>::decode_2718_exact(tx.as_ref()).map_err(AnyError::new)?;
            let signer = tx.try_recover().map_err(AnyError::new)?;
            Ok::<_, AnyError>(tx.with_signer(signer))
        }))
    }
}

#[cfg(test)]
mod osaka_config_tests {
    //! End-to-end verification that [`BerachainEvmConfig`] wires up the Osaka EIPs that
    //! require configuration changes on top of the standard revm Osaka spec (BRIP-0010):
    //!   - EIP-7825 (tx gas cap 2^24)
    //!   - code size bump (32 KB runtime / 64 KB initcode)
    //!   - EIP-7934 (block size cap 10 MiB — sourced from reth-consensus-common)
    use super::*;
    use crate::test_utils::bepolia_chainspec;
    use alloy_genesis::Genesis;
    use jsonrpsee_core::__reexports::serde_json::json;
    use reth::revm::primitives::U256;
    use reth_consensus_common::validation::MAX_RLP_BLOCK_SIZE;
    use reth_primitives_traits::constants::MAX_TX_GAS_LIMIT_OSAKA;

    fn genesis_with_osaka_at(osaka_time: u64) -> Genesis {
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.osaka_time = Some(osaka_time);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1_000_000_000u64,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": { "time": 0, "minimumBaseFeeWei": 0 }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        genesis
    }

    fn config_for_genesis(genesis: Genesis) -> BerachainEvmConfig {
        let chain_spec = Arc::new(BerachainChainSpec::from(genesis));
        BerachainEvmConfig::new_with_evm_factory(chain_spec, BerachainEvmFactory)
    }

    fn header_at(timestamp: u64) -> BerachainHeader {
        BerachainHeader { number: 1, timestamp, base_fee_per_gas: Some(0), ..Default::default() }
    }

    #[test]
    fn test_eip7825_tx_gas_cap_applied_post_osaka() {
        let config = config_for_genesis(genesis_with_osaka_at(1_000));

        let env_post = config.evm_env(&header_at(1_000)).expect("evm_env post-osaka");
        assert_eq!(
            env_post.cfg_env.tx_gas_limit_cap,
            Some(MAX_TX_GAS_LIMIT_OSAKA),
            "tx_gas_limit_cap must be set to MAX_TX_GAS_LIMIT_OSAKA (2^24) when Osaka is active"
        );
        assert_eq!(MAX_TX_GAS_LIMIT_OSAKA, 1 << 24, "Osaka tx gas cap is 2^24 per EIP-7825");

        let env_pre = config.evm_env(&header_at(999)).expect("evm_env pre-osaka");
        assert!(
            env_pre.cfg_env.tx_gas_limit_cap.is_none(),
            "tx_gas_limit_cap must not be set before Osaka"
        );
    }

    #[test]
    fn test_code_size_overrides_applied_post_osaka() {
        let config = config_for_genesis(genesis_with_osaka_at(1_000));

        let env_post = config.evm_env(&header_at(1_000)).expect("evm_env post-osaka");
        assert_eq!(
            env_post.cfg_env.limit_contract_code_size,
            Some(32_768),
            "limit_contract_code_size must be 32 KB under Osaka (BRIP-0010)"
        );
        assert_eq!(
            env_post.cfg_env.limit_contract_initcode_size,
            Some(65_536),
            "limit_contract_initcode_size must be 64 KB under Osaka (BRIP-0010)"
        );

        let env_pre = config.evm_env(&header_at(999)).expect("evm_env pre-osaka");
        assert!(
            env_pre.cfg_env.limit_contract_code_size.is_none(),
            "code size must not be overridden before Osaka"
        );
        assert!(
            env_pre.cfg_env.limit_contract_initcode_size.is_none(),
            "initcode size must not be overridden before Osaka"
        );
    }

    #[test]
    fn test_next_evm_env_applies_osaka_caps() {
        let config = config_for_genesis(genesis_with_osaka_at(1_000));
        let parent = header_at(500);
        let parent_sealed = SealedHeader::new(parent, Default::default());

        let attrs_post = BerachainNextBlockEnvAttributes {
            timestamp: 1_000,
            suggested_fee_recipient: Default::default(),
            prev_randao: Default::default(),
            gas_limit: 30_000_000,
            parent_beacon_block_root: Some(Default::default()),
            withdrawals: Some(Default::default()),
            prev_proposer_pubkey: None,
            extra_data: Default::default(),
        };
        let env = config.next_evm_env(&parent_sealed, &attrs_post).expect("next_evm_env");
        assert_eq!(env.cfg_env.tx_gas_limit_cap, Some(MAX_TX_GAS_LIMIT_OSAKA));
        assert_eq!(env.cfg_env.limit_contract_code_size, Some(32_768));
        assert_eq!(env.cfg_env.limit_contract_initcode_size, Some(65_536));
    }

    #[test]
    fn test_eip7934_max_rlp_block_size_is_8_mib_payload_cap() {
        // BRIP-0010 specifies a 10 MiB block cap with a 2 MiB safety margin, giving an 8 MiB
        // effective payload cap. The engine builder rejects payloads with rlp_length > this value.
        assert_eq!(
            MAX_RLP_BLOCK_SIZE,
            8 * 1024 * 1024,
            "EIP-7934 effective payload cap is 8 MiB (10 MiB - 2 MiB safety margin)"
        );
    }

    #[test]
    fn test_osaka_inactive_before_activation_timestamp() {
        let chain_spec = bepolia_chainspec();
        let config =
            BerachainEvmConfig::new_with_evm_factory(chain_spec.clone(), BerachainEvmFactory);

        // Bepolia activates Osaka at 1_779_897_600; pick the second just before that so
        // we exercise "Osaka not yet active" on the real bepolia chainspec.
        let before_osaka = header_at(1_779_897_599);
        let env = config.evm_env(&before_osaka).expect("evm_env");
        assert!(env.cfg_env.tx_gas_limit_cap.is_none());
        assert!(env.cfg_env.limit_contract_code_size.is_none());
        assert!(env.cfg_env.limit_contract_initcode_size.is_none());
    }
}
