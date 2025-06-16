//! Berachain chain spec

use crate::hardforks::{BerachainHardfork, BerachainHardforks};
use alloy_consensus::BlockHeader;
use alloy_eips::eip2124::{ForkFilter, ForkId, Head};
use alloy_genesis::Genesis;
use derive_more::{Constructor, Into};
use reth::{
    chainspec::{
        BaseFeeParams, Chain, EthereumHardfork, EthereumHardforks, ForkCondition, Hardfork,
    },
    primitives::Header,
    revm::primitives::{Address, B256, U256},
};
use reth_chainspec::{ChainSpec, DepositContract, EthChainSpec, Hardforks};
use reth_cli::chainspec::{ChainSpecParser, parse_genesis};
use reth_ethereum_cli::chainspec::SUPPORTED_CHAINS;
use reth_evm::eth::spec::EthExecutorSpec;
use std::{fmt::Display, sync::Arc};

/// Berachain chain spec
#[derive(Debug, Clone, Into, Constructor, PartialEq, Eq, Default)]
pub struct BerachainChainSpec {
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
        const MIN_BASE_FEE: u64 = 1_000_000_000;

        let raw = parent
            .next_block_base_fee(self.base_fee_params_at_timestamp(parent.timestamp()))
            .unwrap_or_default();

        // enforce at least MIN_BASE_FEE
        raw.max(MIN_BASE_FEE)
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

/// Berachain chain specification parser.
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
    fn from(value: Genesis) -> Self {
        BerachainChainSpec { inner: ChainSpec::from(value) }
    }
}
