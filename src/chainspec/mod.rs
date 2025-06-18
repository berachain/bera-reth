//! Berachain chain specification with Ethereum hardforks plus Prague1 minimum base fee

use crate::{
    genesis::BerachainGenesisConfig,
    hardforks::{BerachainHardfork, BerachainHardforks},
};
use alloy_consensus::BlockHeader;
use alloy_eips::eip2124::{ForkFilter, ForkId, Head};
use alloy_genesis::Genesis;
use derive_more::{Constructor, Into};
use reth::{
    chainspec::{
        BaseFeeParams, Chain, ChainHardforks, EthereumHardfork, EthereumHardforks, ForkCondition,
        Hardfork,
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

    fn next_block_base_fee<H>(&self, parent: &H) -> u64
    where
        Self: Sized,
        H: BlockHeader + BlockHeader,
    {
        let raw = parent
            .next_block_base_fee(self.base_fee_params_at_timestamp(parent.timestamp()))
            .unwrap_or_default();

        // Note that we use this parent block timestamp to determine whether Prague 1 is active.
        // This means that we technically start the base_fee enforcement the block after the fork
        // block. This is a conscious decision to minimize fork diffs across execution clients.
        let min_base_fee = if self.is_prague1_active_at_timestamp(parent.timestamp()) {
            PRAGUE1_MIN_BASE_FEE_WEI
        } else {
            DEFAULT_MIN_BASE_FEE_WEI
        };

        raw.max(min_base_fee)
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
        // Block-based hardforks
        let block_hardfork_opts = [
            (EthereumHardfork::Frontier.boxed(), Some(0)),
            (EthereumHardfork::Homestead.boxed(), genesis.config.homestead_block),
            (EthereumHardfork::Dao.boxed(), genesis.config.dao_fork_block),
            (EthereumHardfork::Tangerine.boxed(), genesis.config.eip150_block),
            (EthereumHardfork::SpuriousDragon.boxed(), genesis.config.eip155_block),
            (EthereumHardfork::Byzantium.boxed(), genesis.config.byzantium_block),
            (EthereumHardfork::Constantinople.boxed(), genesis.config.constantinople_block),
            (EthereumHardfork::Petersburg.boxed(), genesis.config.petersburg_block),
            (EthereumHardfork::Istanbul.boxed(), genesis.config.istanbul_block),
            (EthereumHardfork::MuirGlacier.boxed(), genesis.config.muir_glacier_block),
            (EthereumHardfork::Berlin.boxed(), genesis.config.berlin_block),
            (EthereumHardfork::London.boxed(), genesis.config.london_block),
            (EthereumHardfork::ArrowGlacier.boxed(), genesis.config.arrow_glacier_block),
            (EthereumHardfork::GrayGlacier.boxed(), genesis.config.gray_glacier_block),
        ];
        let mut hardforks = block_hardfork_opts
            .into_iter()
            .filter_map(|(hardfork, opt)| opt.map(|block| (hardfork, ForkCondition::Block(block))))
            .collect::<Vec<_>>();

        // We expect no new networks to be configured with the merge, so we ignore the TTD field
        // and merge netsplit block from external genesis files. All existing networks that have
        // merged should have a static ChainSpec already (namely mainnet and sepolia).
        let paris_block_and_final_difficulty =
            if let Some(ttd) = genesis.config.terminal_total_difficulty {
                hardforks.push((
                    EthereumHardfork::Paris.boxed(),
                    ForkCondition::TTD {
                        // NOTE: this will not work properly if the merge is not activated at
                        // genesis, and there is no merge netsplit block
                        activation_block_number: genesis
                            .config
                            .merge_netsplit_block
                            .unwrap_or_default(),
                        total_difficulty: ttd,
                        fork_block: genesis.config.merge_netsplit_block,
                    },
                ));

                genesis.config.merge_netsplit_block.map(|block| (block, ttd))
            } else {
                None
            };

        // Time-based hardforks
        // For the From implementation, we use a default config if parsing fails
        // This maintains backward compatibility while preventing panics
        let berachain_genesis_config =
            BerachainGenesisConfig::try_from(&genesis.config.extra_fields).unwrap_or_else(|e| {
                tracing::warn!("Failed to parse berachain genesis config, using defaults: {}", e);
                BerachainGenesisConfig::default()
            });

        let time_hardfork_opts = [
            (EthereumHardfork::Shanghai.boxed(), genesis.config.shanghai_time),
            (EthereumHardfork::Cancun.boxed(), genesis.config.cancun_time),
            (EthereumHardfork::Prague.boxed(), genesis.config.prague_time),
            (EthereumHardfork::Osaka.boxed(), genesis.config.osaka_time),
            (BerachainHardfork::Prague1.boxed(), Some(berachain_genesis_config.prague1.time)),
        ];

        let mut time_hardforks = time_hardfork_opts
            .into_iter()
            .filter_map(|(hardfork, opt)| {
                opt.map(|time| (hardfork, ForkCondition::Timestamp(time)))
            })
            .collect::<Vec<_>>();

        hardforks.append(&mut time_hardforks);

        // Ordered Hardforks
        let mainnet_hardforks: ChainHardforks = EthereumHardfork::mainnet().into();
        let mainnet_order = mainnet_hardforks.forks_iter();

        let mut ordered_hardforks = Vec::with_capacity(hardforks.len());
        for (hardfork, _) in mainnet_order {
            if let Some(pos) = hardforks.iter().position(|(e, _)| **e == *hardfork) {
                ordered_hardforks.push(hardforks.remove(pos));
            }
        }

        // append the remaining unknown hardforks to ensure we don't filter any out
        ordered_hardforks.append(&mut hardforks);

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

        let hardforks = ChainHardforks::new(ordered_hardforks);

        let inner = ChainSpec {
            chain: genesis.config.chain_id.into(),
            genesis_header: SealedHeader::new_unhashed(make_genesis_header(&genesis, &hardforks)),
            genesis,
            hardforks,
            paris_block_and_final_difficulty,
            deposit_contract,
            blob_params,
            ..Default::default()
        };
        Self { inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_genesis::Genesis;

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
        let genesis = Genesis::default();
        let chain_spec = BerachainChainSpec::from(genesis);

        // Should create a valid chain spec
        assert_eq!(
            *chain_spec.chain().kind(),
            reth_chainspec::ChainKind::Named(reth_chainspec::NamedChain::Mainnet)
        );
    }
}
