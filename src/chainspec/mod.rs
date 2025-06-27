//! Berachain chain specification with Ethereum hardforks plus Prague1 minimum base fee

use crate::{
    genesis::BerachainGenesisConfig,
    hardforks::{BerachainHardfork, BerachainHardforks},
};
use alloy_consensus::BlockHeader;
use alloy_eips::{
    calc_next_block_base_fee,
    eip2124::{ForkFilter, ForkId, Head},
};
use alloy_genesis::Genesis;
use derive_more::{Constructor, Into};
use reth::{
    chainspec::{
        BaseFeeParams, BaseFeeParamsKind, Chain, ChainHardforks, EthereumHardfork,
        EthereumHardforks, ForkCondition, Hardfork,
    },
    primitives::{Header, SealedHeader},
    revm::primitives::{Address, B256, U256, b256},
};
use reth_chainspec::{ChainSpec, DepositContract, EthChainSpec, Hardforks, make_genesis_header};
use reth_cli::chainspec::{ChainSpecParser, parse_genesis};
use reth_ethereum_cli::chainspec::SUPPORTED_CHAINS;
use reth_evm::eth::spec::EthExecutorSpec;
use std::{fmt::Display, sync::Arc};

/// Minimum base fee enforced after Prague1 hardfork (1 gwei)
const PRAGUE1_MIN_BASE_FEE_WEI: u64 = 1_000_000_000;

/// Default minimum base fee when Prague1 is not active.
const DEFAULT_MIN_BASE_FEE_WEI: u64 = 0;

/// Berachain chain specification wrapping Reth's ChainSpec with Prague1 hardfork
#[derive(Debug, Clone, Into, Constructor, PartialEq, Eq, Default)]
pub struct BerachainChainSpec {
    /// The underlying Reth chain specification
    inner: ChainSpec,
}
impl EthChainSpec for BerachainChainSpec {
    type Header = Header;

    fn chain(&self) -> Chain {
        self.inner.chain()
    }

    fn base_fee_params_at_block(&self, block_number: u64) -> BaseFeeParams {
        self.inner.base_fee_params_at_block(block_number)
    }

    fn base_fee_params_at_timestamp(&self, timestamp: u64) -> BaseFeeParams {
        // Use the inner implementation which respects our configured base_fee_params
        // This will correctly return Prague1 parameters when active
        self.inner.base_fee_params_at_timestamp(timestamp)
    }

    fn blob_params_at_timestamp(&self, timestamp: u64) -> Option<alloy_eips::eip7840::BlobParams> {
        self.inner.blob_params_at_timestamp(timestamp)
    }

    fn deposit_contract(&self) -> Option<&DepositContract> {
        self.inner.deposit_contract()
    }

    fn genesis_hash(&self) -> B256 {
        self.inner.genesis_hash()
    }

    fn prune_delete_limit(&self) -> usize {
        self.inner.prune_delete_limit()
    }

    fn display_hardforks(&self) -> Box<dyn Display> {
        Box::new(self.inner.display_hardforks())
    }

    fn genesis_header(&self) -> &Self::Header {
        self.inner.genesis_header()
    }

    fn genesis(&self) -> &alloy_genesis::Genesis {
        self.inner.genesis()
    }

    fn bootnodes(&self) -> Option<Vec<reth_network_peers::node_record::NodeRecord>> {
        self.inner.bootnodes()
    }

    fn final_paris_total_difficulty(&self) -> Option<U256> {
        self.inner.final_paris_total_difficulty()
    }

    fn next_block_base_fee<H>(&self, parent: &H, _: u64) -> Option<u64>
    where
        Self: Sized,
        H: BlockHeader,
    {
        // Note that we use this parent block timestamp to determine whether Prague 1 is active.
        // This means that we technically start the base_fee changes the block after the fork
        // block. This is a conscious decision to minimize fork diffs across execution clients.
        let raw = calc_next_block_base_fee(
            parent.gas_used(),
            parent.gas_limit(),
            parent.base_fee_per_gas()?,
            self.base_fee_params_at_timestamp(parent.timestamp()),
        );

        let min_base_fee = if self.is_prague1_active_at_timestamp(parent.timestamp()) {
            PRAGUE1_MIN_BASE_FEE_WEI
        } else {
            DEFAULT_MIN_BASE_FEE_WEI
        };
        Some(raw.max(min_base_fee))
    }
}

impl EthereumHardforks for BerachainChainSpec {
    fn ethereum_fork_activation(&self, fork: EthereumHardfork) -> ForkCondition {
        self.inner.ethereum_fork_activation(fork)
    }
}

impl Hardforks for BerachainChainSpec {
    fn fork<H: Hardfork>(&self, fork: H) -> ForkCondition {
        self.inner.fork(fork)
    }

    fn forks_iter(&self) -> impl Iterator<Item = (&dyn Hardfork, ForkCondition)> {
        self.inner.forks_iter()
    }

    fn fork_id(&self, head: &Head) -> ForkId {
        self.inner.fork_id(head)
    }

    fn latest_fork_id(&self) -> ForkId {
        self.inner.latest_fork_id()
    }

    fn fork_filter(&self, head: Head) -> ForkFilter {
        self.inner.fork_filter(head)
    }
}

impl BerachainHardforks for BerachainChainSpec {
    fn berachain_fork_activation(&self, fork: BerachainHardfork) -> ForkCondition {
        self.fork(fork)
    }
}

impl EthExecutorSpec for BerachainChainSpec {
    fn deposit_contract_address(&self) -> Option<Address> {
        self.inner.deposit_contract.map(|deposit_contract| deposit_contract.address)
    }
}

/// Parser for Berachain chain specifications
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct BerachainChainSpecParser;

impl ChainSpecParser for BerachainChainSpecParser {
    type ChainSpec = BerachainChainSpec;

    const SUPPORTED_CHAINS: &'static [&'static str] = SUPPORTED_CHAINS;

    fn parse(s: &str) -> eyre::Result<Arc<Self::ChainSpec>> {
        Ok(Arc::new(parse_genesis(s)?.into()))
    }
}

impl From<Genesis> for BerachainChainSpec {
    fn from(genesis: Genesis) -> Self {
        let berachain_genesis_config =
            BerachainGenesisConfig::try_from(&genesis.config.extra_fields).unwrap_or_else(|e| {
                tracing::warn!("Failed to parse berachain genesis config, using defaults: {}", e);
                BerachainGenesisConfig::default()
            });

        // Berachain networks must start with Cancun at genesis
        if genesis.config.cancun_time != Some(0) {
            panic!(
                "Berachain networks require {} hardfork at genesis (time = 0)",
                EthereumHardfork::Cancun
            );
        }

        // All pre-Cancun forks must be at genesis (block 0)
        let pre_cancun_forks = [
            (EthereumHardfork::Homestead, genesis.config.homestead_block),
            (EthereumHardfork::Dao, genesis.config.dao_fork_block),
            (EthereumHardfork::Tangerine, genesis.config.eip150_block),
            (EthereumHardfork::SpuriousDragon, genesis.config.eip155_block),
            (EthereumHardfork::Byzantium, genesis.config.byzantium_block),
            (EthereumHardfork::Constantinople, genesis.config.constantinople_block),
            (EthereumHardfork::Petersburg, genesis.config.petersburg_block),
            (EthereumHardfork::Istanbul, genesis.config.istanbul_block),
            (EthereumHardfork::MuirGlacier, genesis.config.muir_glacier_block),
            (EthereumHardfork::Berlin, genesis.config.berlin_block),
            (EthereumHardfork::London, genesis.config.london_block),
            (EthereumHardfork::ArrowGlacier, genesis.config.arrow_glacier_block),
            (EthereumHardfork::GrayGlacier, genesis.config.gray_glacier_block),
        ];

        for (hardfork, block) in pre_cancun_forks {
            match block {
                Some(block_num) if block_num != 0 => {
                    panic!(
                        "Berachain networks require {hardfork} hardfork at genesis (block 0), got block {block_num}"
                    );
                }
                _ => {}
            }
        }

        // Shanghai must be at genesis if configured
        match genesis.config.shanghai_time {
            Some(shanghai_time) if shanghai_time != 0 => {
                panic!(
                    "Berachain networks require {} hardfork at genesis (time = 0), got time {shanghai_time}",
                    EthereumHardfork::Shanghai
                );
            }
            _ => {}
        }

        // Validate Prague1 comes after Prague if both are configured
        match (genesis.config.prague_time, berachain_genesis_config.prague1.time) {
            (Some(prague_time), prague1_time) if prague1_time < prague_time => {
                panic!("Prague1 hardfork must activate at or after Prague hardfork");
            }
            _ => {}
        }

        // Berachain networks don't support proof-of-work or non-genesis merge
        if let Some(ttd) = genesis.config.terminal_total_difficulty {
            if !ttd.is_zero() {
                panic!(
                    "Berachain networks require terminal total difficulty of 0 (merge at genesis)"
                );
            }
        } else {
            panic!("Berachain networks require terminal_total_difficulty to be set to 0");
        }
        match genesis.config.merge_netsplit_block {
            Some(merge_block) if merge_block != 0 => {
                panic!(
                    "Berachain networks require merge at genesis (block 0), got block {merge_block}"
                );
            }
            _ => {}
        }

        // Berachain hardforks: all pre-Cancun at genesis, then configurable time-based forks
        let mut hardforks = vec![
            (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Dao.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::SpuriousDragon.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Constantinople.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Petersburg.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::MuirGlacier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::ArrowGlacier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::GrayGlacier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Paris.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Shanghai.boxed(), ForkCondition::Timestamp(0)),
            (EthereumHardfork::Cancun.boxed(), ForkCondition::Timestamp(0)),
        ];

        // Add post-Cancun configurable forks
        if let Some(prague_time) = genesis.config.prague_time {
            hardforks
                .push((EthereumHardfork::Prague.boxed(), ForkCondition::Timestamp(prague_time)));
        }
        if let Some(osaka_time) = genesis.config.osaka_time {
            hardforks.push((EthereumHardfork::Osaka.boxed(), ForkCondition::Timestamp(osaka_time)));
        }
        hardforks.push((
            BerachainHardfork::Prague1.boxed(),
            ForkCondition::Timestamp(berachain_genesis_config.prague1.time),
        ));

        let paris_block_and_final_difficulty =
            Some((0, genesis.config.terminal_total_difficulty.unwrap_or_default()));

        // Extract blob parameters directly from blob_schedule
        let blob_params = genesis.config.blob_schedule_blob_params();

        // NOTE: in full node, we prune all receipts except the deposit contract's. We do not
        // have the deployment block in the genesis file, so we use block zero. We use the same
        // deposit topic as the mainnet contract if we have the deposit contract address in the
        // genesis json.
        let deposit_contract =
            genesis.config.deposit_contract_address.map(|address| DepositContract {
                address,
                block: 0,
                // This value is copied from Reth mainnet. Berachain's deposit contract topic is
                // different but also unused.
                topic: b256!("0x649bbc62d0e31342afea4e5cd82d4049e7e1ee912fc0889aa790803be39038c5"),
            });

        let hardforks = ChainHardforks::new(hardforks);

        // Create base fee parameters based on Prague1 configuration
        let base_fee_params = if berachain_genesis_config.prague1.time == 0 {
            // Prague1 active at genesis - use constant params with Berachain's denominator
            BaseFeeParamsKind::Constant(BaseFeeParams {
                max_change_denominator: berachain_genesis_config
                    .prague1
                    .base_fee_change_denominator,
                elasticity_multiplier: 2, // Standard Ethereum value
            })
        } else {
            // Prague1 activates later - use variable params
            let fork_base_fee_params = vec![
                // Pre-Prague1: standard Ethereum params
                (
                    EthereumHardfork::London.boxed(),
                    BaseFeeParams {
                        max_change_denominator: 8, // Standard Ethereum value
                        elasticity_multiplier: 2,
                    },
                ),
                // Post-Prague1: Berachain params
                (
                    BerachainHardfork::Prague1.boxed(),
                    BaseFeeParams {
                        max_change_denominator: berachain_genesis_config
                            .prague1
                            .base_fee_change_denominator,
                        elasticity_multiplier: 2,
                    },
                ),
            ];
            BaseFeeParamsKind::Variable(fork_base_fee_params.into())
        };

        let inner = ChainSpec {
            chain: genesis.config.chain_id.into(),
            genesis_header: SealedHeader::new_unhashed(make_genesis_header(&genesis, &hardforks)),
            genesis,
            hardforks,
            paris_block_and_final_difficulty,
            deposit_contract,
            blob_params,
            base_fee_params,
            ..Default::default()
        };
        Self { inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_genesis::Genesis;
    use jsonrpsee_core::__reexports::serde_json::json;

    #[test]
    fn test_chain_spec_default() {
        let chain_spec = BerachainChainSpec::default();

        // Test that default creates a valid chain spec
        assert_eq!(chain_spec.prune_delete_limit(), 20000);
        assert!(chain_spec.deposit_contract().is_none());
    }

    #[test]
    fn test_base_fee_params() {
        let chain_spec = BerachainChainSpec::default();

        // Test base fee params
        let params = chain_spec.base_fee_params_at_timestamp(0);
        assert_eq!(params.max_change_denominator, 8);
        assert_eq!(params.elasticity_multiplier, 2);
    }

    #[test]
    fn test_from_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        let chain_spec = BerachainChainSpec::from(genesis);

        // Should create a valid chain spec
        assert_eq!(
            *chain_spec.chain().kind(),
            reth_chainspec::ChainKind::Named(reth_chainspec::NamedChain::Mainnet)
        );
    }

    #[test]
    fn test_base_fee_params_prague1_at_genesis() {
        // Create genesis with Prague1 active at genesis (time = 0)
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0); // Enable EIP-1559
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // At genesis, should use Berachain's base fee params
        let params = chain_spec.base_fee_params_at_timestamp(0);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);

        // Should still be the same after genesis
        let params = chain_spec.base_fee_params_at_timestamp(1000);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);
    }

    #[test]
    fn test_base_fee_params_prague1_delayed() {
        // Create genesis with Prague1 activating at timestamp 1000
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0); // Enable EIP-1559
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // Before Prague1, should use standard Ethereum params
        let params = chain_spec.base_fee_params_at_timestamp(999);
        assert_eq!(params.max_change_denominator, 8);
        assert_eq!(params.elasticity_multiplier, 2);

        // At Prague1 activation, should use Berachain params
        let params = chain_spec.base_fee_params_at_timestamp(1000);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);

        // After Prague1, should still use Berachain params
        let params = chain_spec.base_fee_params_at_timestamp(2000);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);
    }

    #[test]
    fn test_base_fee_params_custom_denominator() {
        // Test with a custom denominator value
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 100,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        let params = chain_spec.base_fee_params_at_timestamp(0);
        assert_eq!(params.max_change_denominator, 100);
        assert_eq!(params.elasticity_multiplier, 2);
    }

    #[test]
    fn test_base_fee_params_missing_berachain_config() {
        // Test fallback when berachain config is missing
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        // No berachain config in extra_fields

        let chain_spec = BerachainChainSpec::from(genesis);

        // Should use default config (Prague1 at time 0, denominator 48)
        let params = chain_spec.base_fee_params_at_timestamp(0);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);
    }

    #[test]
    fn test_prague1_hardfork_activation() {
        // Test that Prague1 hardfork is properly registered
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1500,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // Check Prague1 activation
        assert!(!chain_spec.is_prague1_active_at_timestamp(1499));
        assert!(chain_spec.is_prague1_active_at_timestamp(1500));
        assert!(chain_spec.is_prague1_active_at_timestamp(2000));
    }

    #[test]
    fn test_next_block_base_fee_with_prague1() {
        // Create genesis with Prague1 at timestamp 1000
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0); // Required for Berachain
        genesis.config.terminal_total_difficulty = Some(U256::ZERO); // Required for Berachain
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // Create a parent block before Prague1
        let parent_header =
            Header { timestamp: 999, base_fee_per_gas: Some(100_000_000), ..Default::default() };

        // Before Prague1, base fee can go below 1 gwei
        let next_base_fee = chain_spec.next_block_base_fee(&parent_header, 0);
        assert!(next_base_fee.unwrap() < PRAGUE1_MIN_BASE_FEE_WEI);

        // Create a parent block at Prague1 activation
        let parent_header =
            Header { timestamp: 1000, base_fee_per_gas: Some(100_000_000), ..Default::default() };

        // After Prague1, base fee should be at least 1 gwei
        let next_base_fee = chain_spec.next_block_base_fee(&parent_header, 0);
        assert_eq!(next_base_fee.unwrap(), PRAGUE1_MIN_BASE_FEE_WEI);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require terminal_total_difficulty to be set to 0"
    )]
    fn test_panic_on_missing_ttd() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        // No terminal_total_difficulty set
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Berachain networks require Cancun hardfork at genesis (time = 0)")]
    fn test_panic_on_missing_cancun() {
        let genesis = Genesis::default();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Berachain networks require Cancun hardfork at genesis (time = 0)")]
    fn test_panic_on_cancun_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(100);
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require London hardfork at genesis (block 0), got block 5"
    )]
    fn test_panic_on_london_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.london_block = Some(5);
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require Shanghai hardfork at genesis (time = 0), got time 500"
    )]
    fn test_panic_on_shanghai_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.shanghai_time = Some(500);
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Prague1 hardfork must activate at or after Prague hardfork")]
    fn test_panic_on_prague1_before_prague() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(2000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    fn test_valid_prague1_after_prague() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.prague_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 2000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let chain_spec = BerachainChainSpec::from(genesis);
        assert!(chain_spec.is_prague1_active_at_timestamp(2000));
    }

    #[test]
    fn test_valid_prague1_same_time_as_prague() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.prague_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let chain_spec = BerachainChainSpec::from(genesis);
        assert!(chain_spec.is_prague1_active_at_timestamp(1000));
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require terminal total difficulty of 0 (merge at genesis)"
    )]
    fn test_panic_on_non_zero_ttd() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::from(1000));
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Berachain networks require merge at genesis (block 0), got block 5")]
    fn test_panic_on_merge_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.merge_netsplit_block = Some(5);
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require Dao hardfork at genesis (block 0), got block 5"
    )]
    fn test_panic_on_dao_fork_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.dao_fork_block = Some(5);
        let _chain_spec = BerachainChainSpec::from(genesis);
    }
}
