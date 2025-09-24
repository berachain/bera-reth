//! Berachain chain specification with Ethereum hardforks plus Prague1 minimum base fee

use crate::{
    genesis::BerachainGenesisConfig,
    hardforks::{BerachainHardfork, BerachainHardforks},
    primitives::{BerachainHeader, header::BlsPublicKey},
};
use alloy_consensus::BlockHeader;
use alloy_eips::{
    calc_next_block_base_fee,
    eip2124::{ForkFilter, ForkId, Head},
};
use alloy_genesis::Genesis;
use alloy_primitives::Sealable;
use derive_more::{Constructor, Into};
use reth::{
    chainspec::{
        BaseFeeParams, BaseFeeParamsKind, Chain, ChainHardforks, EthereumHardfork,
        EthereumHardforks, ForkCondition, Hardfork, NamedChain::BerachainBepolia,
    },
    primitives::SealedHeader,
    revm::primitives::{Address, B256, U256, b256},
};
use reth_chainspec::{
    ChainSpec, DepositContract, EthChainSpec, Hardforks, MAINNET_PRUNE_DELETE_LIMIT,
    NamedChain::Berachain, make_genesis_header,
};
use reth_cli::chainspec::{ChainSpecParser, parse_genesis};
use reth_ethereum_cli::chainspec::SUPPORTED_CHAINS;
use reth_evm::eth::spec::EthExecutorSpec;
use std::{fmt::Display, sync::Arc};

/// Default minimum base fee when Prague1 is not active.
const DEFAULT_MIN_BASE_FEE_WEI: u64 = 0;

/// Berachain chain specification wrapping Reth's ChainSpec with Berachain hardforks
#[derive(Debug, Clone, Into, Constructor, PartialEq, Eq, Default)]
pub struct BerachainChainSpec {
    /// The underlying Reth chain specification
    pub inner: ChainSpec,
    pub genesis_header: BerachainHeader,
    /// PoL contract address loaded from configuration
    pub pol_contract_address: Address,
    /// The minimum base fee in wei for Prague1
    pub prague1_minimum_base_fee: u64,
    /// The minimum base fee in wei for Prague2
    pub prague2_minimum_base_fee: u64,
}

impl BerachainChainSpec {
    pub fn pol_contract(&self) -> Address {
        self.pol_contract_address
    }
}

impl EthChainSpec for BerachainChainSpec {
    type Header = BerachainHeader;

    fn chain(&self) -> Chain {
        // Required for etherscan integration (--debug.etherscan) to work correctly
        // Maps chain IDs to their corresponding NamedChain variants
        match self.inner.chain_id() {
            id if id == (Berachain as u64) => Chain::from(Berachain),
            id if id == (BerachainBepolia as u64) => Chain::from(BerachainBepolia),
            _ => self.inner.chain(),
        }
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
        self.genesis_header.hash_slow()
    }

    fn prune_delete_limit(&self) -> usize {
        self.inner.prune_delete_limit()
    }

    fn display_hardforks(&self) -> Box<dyn Display> {
        let inner_display = self.inner.display_hardforks().to_string();

        let prague1_details = match self.fork(BerachainHardfork::Prague1) {
            ForkCondition::Timestamp(time) => {
                let base_fee_params = self.base_fee_params_at_timestamp(time);
                format!(
                    "\nBerachain Prague1 configuration: {{time={}, base_fee_denominator={}, min_base_fee={} gwei, pol_distributor={}}}",
                    time,
                    base_fee_params.max_change_denominator,
                    self.prague1_minimum_base_fee / 1_000_000_000,
                    self.pol_contract_address
                )
            }
            _ => "\nPrague1 Misconfigured".to_string(),
        };

        let prague2_details = match self.fork(BerachainHardfork::Prague2) {
            ForkCondition::Timestamp(time) => {
                format!(
                    "\nBerachain Prague2 configuration: {{time={}, min_base_fee={} gwei}}",
                    time,
                    self.prague2_minimum_base_fee / 1_000_000_000
                )
            }
            _ => "\nPrague2 Misconfigured".to_string(),
        };

        Box::new(format!("{inner_display}{prague1_details}{prague2_details}"))
    }

    fn genesis_header(&self) -> &Self::Header {
        &self.genesis_header
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

    fn next_block_base_fee(&self, parent: &Self::Header, _: u64) -> Option<u64> {
        // Note that we use this parent block timestamp to determine whether Prague2/1 is active.
        // This means that we technically start the base_fee changes the block after the fork
        // block. This is a conscious decision to minimize fork diffs across execution clients.
        let raw = calc_next_block_base_fee(
            parent.gas_used(),
            parent.gas_limit(),
            parent.base_fee_per_gas()?,
            self.base_fee_params_at_timestamp(parent.timestamp()),
        );

        // Prague2 supersedes Prague1 - check Prague2 first
        let min_base_fee = if self.is_prague2_active_at_timestamp(parent.timestamp()) {
            self.prague2_minimum_base_fee
        } else if self.is_prague1_active_at_timestamp(parent.timestamp()) {
            self.prague1_minimum_base_fee
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

impl BerachainChainSpec {
    /// Create a BerachainChainSpec that fallbacks to Ethereum behavior
    fn ethereum_fallback(genesis: Genesis) -> Self {
        let mut inner = ChainSpec::from(genesis);
        // This is added to prevent hive tests from failing as bera-reth will think it's
        // an optimism chain and fail on startup when the ChainId is 10.
        if inner.chain.is_optimism() {
            inner.chain = Chain::from_id_unchecked(inner.chain.id());
        }
        let genesis_header = BerachainHeader::from(inner.genesis_header());
        Self {
            inner,
            genesis_header,
            pol_contract_address: Address::ZERO,
            prague1_minimum_base_fee: 0,
            prague2_minimum_base_fee: 0,
        }
    }
}

impl From<Genesis> for BerachainChainSpec {
    /// Intentionally panics if required fields are missing from genesis or invalid.
    fn from(genesis: Genesis) -> Self {
        let berachain_genesis_config =
            BerachainGenesisConfig::try_from(&genesis.config.extra_fields).unwrap_or_else(|e| {
                panic!("Failed to parse berachain genesis config: {e}. Please ensure the genesis file contains a valid 'berachain' configuration section");
            });

        // If not a berachain genesis, fallback to Ethereum behavior
        if !berachain_genesis_config.is_berachain() {
            return Self::ethereum_fallback(genesis);
        }

        // Parse Prague1 and Prague2 configurations if present
        let prague1_config_opt = berachain_genesis_config.prague1;
        let prague2_config_opt = berachain_genesis_config.prague2;

        // Both Prague1 and Prague2 are required for Berachain genesis
        let (prague1_config, prague2_config) = match (prague1_config_opt, prague2_config_opt) {
            (Some(p1), Some(p2)) => (p1, p2),
            (_, _) => {
                panic!("Berachain networks require Prague1 and Prague2 hardforks to be configured")
            }
        };

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
        match (genesis.config.prague_time, prague1_config.time) {
            (Some(prague_time), prague1_time) if prague1_time < prague_time => {
                panic!(
                    "Prague1 hardfork must activate at or after Prague hardfork. Prague time: {prague_time}, Prague1 time: {prague1_time}. Check that Prague1 time is not malformed (should be a valid Unix timestamp).",
                );
            }
            (None, _) => {
                panic!("Prague1 hardfork requires Prague hardfork to be configured");
            }
            _ => {}
        }

        // Validate Prague2 ordering (Prague2 must come at or after Prague1)
        // Transitivity: if Prague1 >= Prague and Prague2 >= Prague1, then Prague2 >= Prague
        if prague2_config.time < prague1_config.time {
            panic!(
                "Prague2 hardfork must activate at or after Prague1 hardfork. Prague1 time: {}, Prague2 time: {}. Check that Prague2 time is not malformed (should be a valid Unix timestamp).",
                prague1_config.time, prague2_config.time
            );
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

        // Berachain networks do not support BPO hardforks - enforce they are not configured
        let bpo_forks = [
            ("bpo1_time", genesis.config.bpo1_time),
            ("bpo2_time", genesis.config.bpo2_time),
            ("bpo3_time", genesis.config.bpo3_time),
            ("bpo4_time", genesis.config.bpo4_time),
            ("bpo5_time", genesis.config.bpo5_time),
        ];

        for (fork_name, fork_time) in bpo_forks {
            if fork_time.is_some() {
                panic!(
                    "Berachain networks do not support BPO hardforks. Found {fork_name} configured in genesis."
                );
            }
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

        // Add Prague1 and Prague2 hardforks (both always configured)
        hardforks.push((
            BerachainHardfork::Prague1.boxed(),
            ForkCondition::Timestamp(prague1_config.time),
        ));
        hardforks.push((
            BerachainHardfork::Prague2.boxed(),
            ForkCondition::Timestamp(prague2_config.time),
        ));

        if let Some(osaka_time) = genesis.config.osaka_time {
            hardforks.push((EthereumHardfork::Osaka.boxed(), ForkCondition::Timestamp(osaka_time)));
        }

        let paris_block_and_final_difficulty =
            Some((0, genesis.config.terminal_total_difficulty.unwrap()));

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

        // Create base fee parameters
        let ethereum_basefee_params = BaseFeeParams::ethereum();
        let base_fee_params = BaseFeeParamsKind::Variable(
            vec![
                (
                    EthereumHardfork::London.boxed(),
                    BaseFeeParams {
                        max_change_denominator: ethereum_basefee_params.max_change_denominator,
                        elasticity_multiplier: ethereum_basefee_params.elasticity_multiplier,
                    },
                ),
                (
                    BerachainHardfork::Prague1.boxed(),
                    BaseFeeParams {
                        max_change_denominator: prague1_config.base_fee_change_denominator,
                        elasticity_multiplier: ethereum_basefee_params.elasticity_multiplier,
                    },
                ),
                (
                    BerachainHardfork::Prague2.boxed(),
                    BaseFeeParams {
                        // We use the prague1 base_fee_change_denominator for prague2
                        max_change_denominator: prague1_config.base_fee_change_denominator,
                        elasticity_multiplier: ethereum_basefee_params.elasticity_multiplier,
                    },
                ),
            ]
            .into(),
        );

        let inner = ChainSpec {
            chain: Chain::from_id_unchecked(genesis.config.chain_id),
            genesis_header: SealedHeader::new_unhashed(make_genesis_header(&genesis, &hardforks)),
            genesis: genesis.clone(),
            hardforks,
            paris_block_and_final_difficulty,
            deposit_contract,
            blob_params,
            base_fee_params,
            prune_delete_limit: MAINNET_PRUNE_DELETE_LIMIT,
        };

        let mut genesis_header = BerachainHeader::from(inner.genesis_header());

        // Set prev_proposer_pubkey to zero if Prague1 is active at genesis timestamp
        let is_prague1_at_genesis = prague1_config.time <= genesis.timestamp;
        if is_prague1_at_genesis {
            genesis_header.prev_proposer_pubkey = Some(BlsPublicKey::ZERO);
        }

        // Extract configuration values from Prague1 and Prague2 configs
        let pol_contract_address = prague1_config.pol_distributor_address;
        let prague1_minimum_base_fee = prague1_config.minimum_base_fee_wei;
        let prague2_minimum_base_fee = prague2_config.minimum_base_fee_wei;

        Self {
            inner,
            genesis_header,
            pol_contract_address,
            prague1_minimum_base_fee,
            prague2_minimum_base_fee,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_genesis::Genesis;
    use alloy_primitives::address;
    use jsonrpsee_core::__reexports::serde_json::json;
    use reth_chainspec::ForkHash;

    #[test]
    fn test_deposit_contract_default_regression() {
        let chain_spec = BerachainChainSpec::default();
        assert!(chain_spec.deposit_contract().is_none());
    }

    #[test]
    fn test_from_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let chain_spec = BerachainChainSpec::from(genesis);

        // Should create a valid chain spec
        assert_eq!(*chain_spec.chain().kind(), reth_chainspec::ChainKind::Id(1));
    }

    #[test]
    fn test_base_fee_params_prague1_at_genesis() {
        // Create genesis with Prague1 active at genesis (time = 0)
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0); // Enable EIP-1559
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // At genesis, should use prague1 base fee params
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
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(500);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 2000,
                    "minimumBaseFeeWei": 0
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

        // Between Prague1 and Prague2, should still use Prague1 params
        let params = chain_spec.base_fee_params_at_timestamp(1999);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);

        // At Prague2 activation, should inherit Prague1's denominator but change minimum base fee
        let params = chain_spec.base_fee_params_at_timestamp(2000);
        assert_eq!(params.max_change_denominator, 48);
        assert_eq!(params.elasticity_multiplier, 2);

        // Verify minimum base fee behavior changes across hardforks
        assert_eq!(chain_spec.prague1_minimum_base_fee, 1000000000); // 1 gwei
        assert_eq!(chain_spec.prague2_minimum_base_fee, 0); // 0 wei
        assert!(!chain_spec.is_prague1_active_at_timestamp(999));
        assert!(chain_spec.is_prague1_active_at_timestamp(1000));
        assert!(!chain_spec.is_prague2_active_at_timestamp(1999));
        assert!(chain_spec.is_prague2_active_at_timestamp(2000));
    }

    #[test]
    fn test_base_fee_params_custom_denominator() {
        // Test with a custom denominator value and verify Prague2 inherits it
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 500,
                    "baseFeeChangeDenominator": 100,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 1000,
                    "minimumBaseFeeWei": 0,
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // Before Prague1, should use Ethereum default (8)
        let params = chain_spec.base_fee_params_at_timestamp(499);
        assert_eq!(params.max_change_denominator, 8);
        assert_eq!(params.elasticity_multiplier, 2);

        // At Prague1, should use custom denominator (100)
        let params = chain_spec.base_fee_params_at_timestamp(500);
        assert_eq!(params.max_change_denominator, 100);
        assert_eq!(params.elasticity_multiplier, 2);

        // At Prague2, should inherit Prague1's custom denominator (100)
        let params = chain_spec.base_fee_params_at_timestamp(1000);
        assert_eq!(params.max_change_denominator, 100);
        assert_eq!(params.elasticity_multiplier, 2);

        // Verify Prague2 inherits Prague1's denominator but has different minimum base fee
        assert_eq!(chain_spec.prague1_minimum_base_fee, 1000000000); // 1 gwei
        assert_eq!(chain_spec.prague2_minimum_base_fee, 0); // 0 wei
    }

    #[test]
    fn test_default_prune_delete_limit_is_20000() {
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 8,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0,
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        assert_eq!(chain_spec.prune_delete_limit(), 20000);
        assert_eq!(chain_spec.inner.prune_delete_limit, 20000);
    }

    #[test]
    fn test_base_fee_params_missing_berachain_config() {
        // Test fallback to Ethereum behavior when berachain config is missing
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        // No berachain config in extra_fields - should fallback to Ethereum behavior

        let chain_spec = BerachainChainSpec::from(genesis);

        // Should have default values for Berachain-specific fields
        assert_eq!(chain_spec.pol_contract_address, Address::ZERO);
        assert_eq!(chain_spec.prague1_minimum_base_fee, 0);
        assert!(!chain_spec.is_prague1_active_at_timestamp(0));
        assert!(!chain_spec.is_prague2_active_at_timestamp(0));
        assert!(!chain_spec.is_prague1_active_at_timestamp(u64::MAX));
        assert!(!chain_spec.is_prague2_active_at_timestamp(u64::MAX));
    }

    #[test]
    #[should_panic(expected = "Failed to parse berachain genesis config")]
    fn test_missing_pol_distributor_address() {
        // Test panic when polDistributorAddress is missing
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                    // Missing polDistributorAddress - should panic
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    fn test_prague1_and_prague2_hardfork_activation() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(1500);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1500,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 3000,
                    "minimumBaseFeeWei": 0
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

        // Check Prague2 activation
        assert!(!chain_spec.is_prague2_active_at_timestamp(2999));
        assert!(chain_spec.is_prague2_active_at_timestamp(3000));
        assert!(chain_spec.is_prague2_active_at_timestamp(3500));
    }

    #[test]
    fn test_next_block_base_fee_across_hardforks() {
        let prague1_base_fee = 10_000_000_000;
        let prague2_base_fee = 1000;
        let mut genesis = Genesis::default();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(1000);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);

        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 10000000000i64,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 2000,
                    "minimumBaseFeeWei": 1000
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // Create a parent block before Prague1
        let parent_header = BerachainHeader {
            timestamp: 999,
            base_fee_per_gas: Some(100_000_000),
            ..Default::default()
        };

        // Before Prague1, base fee can go below 10 gwei
        let next_base_fee = chain_spec.next_block_base_fee(&parent_header, 0).unwrap();
        assert!(next_base_fee < prague1_base_fee);

        // Create a parent block at Prague1 activation
        let parent_header = BerachainHeader {
            timestamp: 1000,
            base_fee_per_gas: Some(100_000_000),
            ..Default::default()
        };

        // After Prague1, base fee should be at least 10 gwei
        let next_base_fee = chain_spec.next_block_base_fee(&parent_header, 0).unwrap();
        assert_eq!(next_base_fee, prague1_base_fee);

        // Create a parent block before Prague2 activation
        let parent_header =
            BerachainHeader { timestamp: 1999, base_fee_per_gas: Some(0), ..Default::default() };

        let next_base_fee = chain_spec.next_block_base_fee(&parent_header, 0).unwrap();
        assert_eq!(next_base_fee, prague1_base_fee);

        // Create a parent block at Prague2 activation
        let parent_header =
            BerachainHeader { timestamp: 2000, base_fee_per_gas: Some(0), ..Default::default() };

        let next_base_fee = chain_spec.next_block_base_fee(&parent_header, 0).unwrap();
        assert_eq!(next_base_fee, prague2_base_fee);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require terminal_total_difficulty to be set to 0"
    )]
    fn test_panic_on_missing_ttd() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        // No terminal_total_difficulty set
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Berachain networks require Cancun hardfork at genesis (time = 0)")]
    fn test_panic_on_missing_cancun() {
        let mut genesis = Genesis::default();
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        // No cancun_time set - should panic
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Berachain networks require Cancun hardfork at genesis (time = 0)")]
    fn test_panic_on_cancun_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(100);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require London hardfork at genesis (block 0), got block 5"
    )]
    fn test_panic_on_london_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.london_block = Some(5);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require Shanghai hardfork at genesis (time = 0), got time 500"
    )]
    fn test_panic_on_shanghai_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.shanghai_time = Some(500);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
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
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 3000,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Prague1 hardfork requires Prague hardfork to be configured")]
    fn test_panic_on_prague1_without_prague() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 1000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 2000,
                    "minimumBaseFeeWei": 0
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
        genesis.config.prague_time = Some(1000);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 2000,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 4000,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let chain_spec = BerachainChainSpec::from(genesis);
        assert!(chain_spec.is_prague1_active_at_timestamp(2000));
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require terminal total difficulty of 0 (merge at genesis)"
    )]
    fn test_panic_on_non_zero_ttd() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::from(1000));
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Berachain networks require merge at genesis (block 0), got block 5")]
    fn test_panic_on_merge_not_at_genesis() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.merge_netsplit_block = Some(5);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
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
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(expected = "Failed to parse berachain genesis config")]
    fn test_invalid_base_fee_denominator() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 0,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    fn test_next_block_base_fee_with_none_parent() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let chain_spec = BerachainChainSpec::from(genesis);

        let parent_header =
            BerachainHeader { timestamp: 0, base_fee_per_gas: None, ..Default::default() };

        let result = chain_spec.next_block_base_fee(&parent_header, 0);
        assert!(result.is_none()); // Correctly returns None when parent has no base fee
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks require both Prague1 and Prague2 hardforks to be configured"
    )]
    fn test_prague1_not_enabled_empty_berachain() {
        // Empty berachain config should now panic since both Prague1 and Prague2 are required
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {}
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    fn test_prague1_empty_should_panic() {
        // Prague1 present but empty - should panic
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {}
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        // This should panic because prague1 is present but empty/misconfigured
        std::panic::catch_unwind(|| {
            let _chain_spec = BerachainChainSpec::from(genesis);
        })
        .expect_err("Should panic when prague1 is present but empty");
    }

    #[test]
    #[should_panic(
        expected = "Failed to parse berachain genesis config: Invalid berachain configuration: missing field `polDistributorAddress`"
    )]
    fn test_prague1_missing_fields_should_panic() {
        // Prague1 present but missing required fields - should panic
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000
                    // Missing polDistributorAddress - should panic
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        // This should panic because prague1 is present but misconfigured
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    fn test_prague1_not_enabled_no_berachain_field() {
        // Prague1 not enabled - no berachain field at all (already covered by existing test)
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        // No berachain config in extra_fields

        let chain_spec = BerachainChainSpec::from(genesis);

        // Should fallback to Ethereum behavior
        assert_eq!(chain_spec.pol_contract_address, Address::ZERO);
        assert_eq!(chain_spec.prague1_minimum_base_fee, 0);
        assert!(!chain_spec.is_prague1_active_at_timestamp(u64::MAX));
    }

    #[test]
    fn test_berachain_forks_enabled_at_genesis_valid_config() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 10000000000u64,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 1000000000,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        // Should use Berachain configuration
        assert_eq!(
            chain_spec.pol_contract_address,
            address!("0x4200000000000000000000000000000000000042")
        );
        assert_eq!(chain_spec.prague1_minimum_base_fee, 10000000000);
        assert_eq!(chain_spec.prague2_minimum_base_fee, 0);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks do not support BPO hardforks. Found bpo1_time configured in genesis."
    )]
    fn test_panic_on_bpo1_hardfork() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.bpo1_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks do not support BPO hardforks. Found bpo2_time configured in genesis."
    )]
    fn test_panic_on_bpo2_hardfork() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.bpo2_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks do not support BPO hardforks. Found bpo3_time configured in genesis."
    )]
    fn test_panic_on_bpo3_hardfork() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.bpo3_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks do not support BPO hardforks. Found bpo4_time configured in genesis."
    )]
    fn test_panic_on_bpo4_hardfork() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.bpo4_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    #[should_panic(
        expected = "Berachain networks do not support BPO hardforks. Found bpo5_time configured in genesis."
    )]
    fn test_panic_on_bpo5_hardfork() {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.bpo5_time = Some(1000);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();
        let _chain_spec = BerachainChainSpec::from(genesis);
    }

    #[test]
    fn test_bepolia_fixture() {
        let bepolia_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bepolia-genesis.json");
        let bepolia_json = std::fs::read_to_string(bepolia_path).unwrap();
        let genesis: Genesis = serde_json::from_str(&bepolia_json).unwrap();
        let chain_spec = BerachainChainSpec::from(genesis);

        // Verify Berachain-specific configuration from bepolia fixture
        assert_eq!(
            chain_spec.pol_contract_address,
            address!("D2f19a79b026Fb636A7c300bF5947df113940761")
        );
        assert_eq!(chain_spec.prague1_minimum_base_fee, 10_000_000_000); // 10 gwei
        assert_eq!(chain_spec.prague2_minimum_base_fee, 0); // 0 gwei
        assert_eq!(chain_spec.inner.chain.id(), 80069); // bepolia chain id

        // Prague1 should be active after timestamp 1754496000
        assert!(!chain_spec.is_prague1_active_at_timestamp(1754495999));
        assert!(chain_spec.is_prague1_active_at_timestamp(1754496000));
    }

    #[test]
    fn test_chain_uses_id_not_named() {
        let mut genesis = Genesis::default();
        genesis.config.chain_id = 10;
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        let extra_fields_json = json!({
            "berachain": {
                "prague1": {
                    "time": 0,
                    "baseFeeChangeDenominator": 48,
                    "minimumBaseFeeWei": 1000000000,
                    "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                },
                "prague2": {
                    "time": 0,
                    "minimumBaseFeeWei": 0
                }
            }
        });
        genesis.config.extra_fields =
            reth::rpc::types::serde_helpers::OtherFields::try_from(extra_fields_json).unwrap();

        let chain_spec = BerachainChainSpec::from(genesis);

        assert_eq!(chain_spec.inner.chain.id(), 10);
        assert!(!chain_spec.inner.chain.is_optimism());
    }

    #[test]
    fn test_ethereum_fallback_overrides_optimism_only() {
        let mut genesis = Genesis::default();
        genesis.config.chain_id = 10;
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);

        let chain_spec = BerachainChainSpec::from(genesis);

        assert_eq!(chain_spec.inner.chain.id(), 10);
        assert!(!chain_spec.inner.chain.is_optimism());
    }

    #[test]
    fn test_ethereum_fallback_preserves_non_optimism_chains() {
        let mut genesis = Genesis::default();
        genesis.config.chain_id = 1; // Ethereum mainnet
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);

        let chain_spec = BerachainChainSpec::from(genesis);

        assert_eq!(chain_spec.inner.chain.id(), 1);
        assert!(chain_spec.inner.chain.is_ethereum());
    }

    #[test]
    fn test_ethereum_base_fee_params_regression() {
        // Regression test to ensure Ethereum base fee parameters maintain expected values
        let ethereum_params = BaseFeeParams::ethereum();

        assert_eq!(
            ethereum_params.max_change_denominator, 8,
            "Ethereum max_change_denominator should be 8"
        );
        assert_eq!(
            ethereum_params.elasticity_multiplier, 2,
            "Ethereum elasticity_multiplier should be 2"
        );
    }

    #[test]
    fn test_fork_id_unchanged_with_genesis_config() {
        let create_genesis = |prague1_time: u64, prague2_time: u64| {
            let mut genesis = Genesis::default();
            genesis.config.cancun_time = Some(0);
            genesis.config.prague_time = Some(0);
            genesis.config.terminal_total_difficulty = Some(U256::ZERO);
            genesis.config.extra_fields =
                reth::rpc::types::serde_helpers::OtherFields::try_from(json!({
                    "berachain": {
                        "prague1": {
                            "time": prague1_time,
                            "baseFeeChangeDenominator": 48,
                            "minimumBaseFeeWei": 1000000000,
                            "polDistributorAddress": "0x4200000000000000000000000000000000000042"
                        },
                        "prague2": {
                            "time": prague2_time,
                            "minimumBaseFeeWei": 0
                        }
                    }
                }))
                .unwrap();
            genesis
        };

        let spec1 = BerachainChainSpec::from(create_genesis(0, 0));
        let spec2 = BerachainChainSpec::from(create_genesis(0, 1000));
        let spec3 = BerachainChainSpec::from(create_genesis(1000, 2000));
        let spec4 = BerachainChainSpec::from(create_genesis(3000, 4000));

        let head = Head {
            number: 0,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 0,
        };

        let fork_id1 = spec1.fork_id(&head);
        let fork_id2 = spec2.fork_id(&head);
        let fork_id3 = spec3.fork_id(&head);
        let fork_id4 = spec4.fork_id(&head);

        assert_eq!(fork_id1.hash, ForkHash([0xc3, 0x84, 0x31, 0xb9]));
        assert_eq!(fork_id2.hash, ForkHash([0xc3, 0x84, 0x31, 0xb9]));
        assert_eq!(fork_id3.hash, ForkHash([0xc3, 0x84, 0x31, 0xb9]));
        assert_eq!(fork_id4.hash, ForkHash([0xc3, 0x84, 0x31, 0xb9]));
    }

    #[test]
    fn test_bepolia_fork_ids() {
        // Load the actual Bepolia genesis configuration
        let bepolia_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bepolia-genesis.json");
        let bepolia_json = std::fs::read_to_string(bepolia_path).unwrap();
        let genesis: Genesis = serde_json::from_str(&bepolia_json).unwrap();
        let spec = BerachainChainSpec::from(genesis);

        // Test fork ID evolution through different timestamps matching bera-geth test

        // Before Prague fork (Bepolia has Prague at 1746633600)
        let head_before_prague = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1746633599,
        };

        // After Prague, before Prague1 (Bepolia has Prague1 at 1754496000)
        let head_prague_active = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1746633600,
        };

        // After Prague1, before Prague2 (Bepolia has Prague2 at 1758124800)
        let head_prague1_active = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1754496000,
        };

        // After Prague2
        let head_prague2_active = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1758124800,
        };

        let fork_id_before_prague = spec.fork_id(&head_before_prague);
        let fork_id_prague = spec.fork_id(&head_prague_active);
        let fork_id_prague1 = spec.fork_id(&head_prague1_active);
        let fork_id_prague2 = spec.fork_id(&head_prague2_active);

        // Verify next fork schedule matches Bepolia configuration
        assert_eq!(fork_id_before_prague.next, 1746633600, "next fork should be Prague");
        assert_eq!(fork_id_prague.next, 1754496000, "next fork should be Prague1");
        assert_eq!(fork_id_prague1.next, 1758124800, "next fork should be Prague2");
        assert_eq!(fork_id_prague2.next, 0, "no next fork after Prague2");

        // Expected fork hash values for Bepolia (matching bera-geth test values)
        assert_eq!(fork_id_before_prague.hash, ForkHash([0xae, 0x79, 0x53, 0x0c]));
        assert_eq!(fork_id_prague.hash, ForkHash([0xd0, 0x7d, 0x9f, 0x27]));
        assert_eq!(fork_id_prague1.hash, ForkHash([0x33, 0x15, 0x3c, 0x0a]));
        assert_eq!(fork_id_prague2.hash, ForkHash([0x2e, 0xdd, 0x8d, 0x57]));
    }

    #[test]
    fn test_mainnet_fork_ids() {
        // Load the actual Mainnet genesis configuration
        let mainnet_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mainnet-genesis.json");
        let mainnet_json = std::fs::read_to_string(mainnet_path).unwrap();
        let genesis: Genesis = serde_json::from_str(&mainnet_json).unwrap();
        let spec = BerachainChainSpec::from(genesis);

        // Test cases matching bera-geth mainnet test:
        // Prague at 1749056400, Prague1 at 1756915200, Prague2 at 1759248000

        // Genesis state - all forks active except Prague/Prague1/Prague2
        let head_genesis = Head {
            number: 0,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 0,
        };

        // Before Prague fork
        let head_before_prague = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1749056399,
        };

        // Prague active, before Prague1
        let head_prague_active = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1749056400,
        };

        // Prague1 active, before Prague2
        let head_prague1_active = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1756915200,
        };

        // Prague2 active
        let head_prague2_active = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 1759248000,
        };

        // Far future
        let head_far_future = Head {
            number: 1000,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 2000000000,
        };

        // Calculate fork IDs
        let fork_id_genesis = spec.fork_id(&head_genesis);
        let fork_id_before_prague = spec.fork_id(&head_before_prague);
        let fork_id_prague = spec.fork_id(&head_prague_active);
        let fork_id_prague1 = spec.fork_id(&head_prague1_active);
        let fork_id_prague2 = spec.fork_id(&head_prague2_active);
        let fork_id_future = spec.fork_id(&head_far_future);

        // Verify next fork schedule matches mainnet configuration
        assert_eq!(fork_id_genesis.next, 1749056400, "next fork should be Prague");
        assert_eq!(fork_id_before_prague.next, 1749056400, "next fork should be Prague");
        assert_eq!(fork_id_prague.next, 1756915200, "next fork should be Prague1");
        assert_eq!(fork_id_prague1.next, 1759248000, "next fork should be Prague2");
        assert_eq!(fork_id_prague2.next, 0, "no next fork after Prague2");
        assert_eq!(fork_id_future.next, 0, "no next fork in far future");

        // Expected fork hash values for mainnet (matching bera-geth test values)
        assert_eq!(fork_id_genesis.hash, ForkHash([0xbb, 0x6c, 0x8b, 0xc0]));
        assert_eq!(fork_id_before_prague.hash, ForkHash([0xbb, 0x6c, 0x8b, 0xc0]));
        assert_eq!(fork_id_prague.hash, ForkHash([0x3f, 0x78, 0xb1, 0x27]));
        assert_eq!(fork_id_prague1.hash, ForkHash([0xd2, 0xeb, 0xec, 0xac]));
        assert_eq!(fork_id_prague2.hash, ForkHash([0xcb, 0xbf, 0x6c, 0x9f]));
        assert_eq!(fork_id_future.hash, ForkHash([0xcb, 0xbf, 0x6c, 0x9f]));
    }

    #[test]
    fn test_latest_fork_id_matches_final_state() {
        // Load the actual Bepolia genesis configuration
        let bepolia_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bepolia-genesis.json");
        let bepolia_json = std::fs::read_to_string(bepolia_path).unwrap();
        let genesis: Genesis = serde_json::from_str(&bepolia_json).unwrap();
        let spec = BerachainChainSpec::from(genesis);

        let latest_fork_id = spec.latest_fork_id();

        // Create a head far in the future (after all Prague2 activation at 1758124800)
        let head_final = Head {
            number: 100,
            hash: B256::ZERO,
            difficulty: Default::default(),
            total_difficulty: Default::default(),
            timestamp: 2000000000, // Far future
        };
        let current_fork_id = spec.fork_id(&head_final);

        assert_eq!(
            latest_fork_id.hash, current_fork_id.hash,
            "latest_fork_id should match fork_id at final state"
        );
        assert_eq!(latest_fork_id.next, 0, "latest fork should have no next fork");

        // Verify this matches the final Prague2 state from bera-geth test
        assert_eq!(latest_fork_id.hash, ForkHash([0x2e, 0xdd, 0x8d, 0x57]));
    }
}
