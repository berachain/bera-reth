//! Berachain chain configuration handler for EIP-7910.

use crate::{
    chainspec::BerachainChainSpec, node::evm::config::BerachainEvmConfig,
    primitives::BerachainHeader,
};
use alloy_eips::eip7910::{EthConfig, EthForkConfig, SystemContract};
use alloy_primitives::Address;
use jsonrpsee::core::RpcResult;
use reth::{providers::BlockReaderIdExt, revm::database_interface::EmptyDB};
use reth_chainspec::{ChainSpecProvider, EthChainSpec, EthereumHardforks, Hardforks, Head};
use reth_errors::{ProviderError, RethError};
use reth_evm::{
    ConfigureEvm, Evm,
    precompiles::{Precompile, PrecompilesMap},
};
use reth_rpc_eth_api::helpers::config::EthConfigApiServer;
use reth_rpc_eth_types::EthApiError;

use std::collections::BTreeMap;

/// EIP-7910 `systemContracts` key for the PoL distributor (BRIP-0004 / Prague1).
const POL_DISTRIBUTOR_SYSTEM_CONTRACT: &str = "POL_DISTRIBUTOR_ADDRESS";

/// Berachain `eth_config` RPC handler implementing EIP-7910.
#[derive(Debug, Clone)]
pub struct BerachainConfigHandler<Provider> {
    provider: Provider,
    evm_config: BerachainEvmConfig,
}

impl<Provider> BerachainConfigHandler<Provider>
where
    Provider: ChainSpecProvider<ChainSpec = BerachainChainSpec>
        + BlockReaderIdExt<Header = BerachainHeader>
        + 'static,
{
    /// Creates a new handler.
    pub const fn new(provider: Provider, evm_config: BerachainEvmConfig) -> Self {
        Self { provider, evm_config }
    }

    /// Builds fork config for timestamp, returns None if no blob params exist.
    fn build_fork_config_at(
        &self,
        fork_timestamp: u64,
        precompiles: BTreeMap<String, Address>,
        system_contracts_eval_timestamp: u64,
    ) -> Option<EthForkConfig> {
        let chain_spec = self.provider.chain_spec();

        let mut system_contracts = BTreeMap::<SystemContract, Address>::default();

        if chain_spec.is_cancun_active_at_timestamp(fork_timestamp) {
            system_contracts.extend(SystemContract::cancun());
        }

        if chain_spec.is_prague_active_at_timestamp(fork_timestamp) {
            system_contracts
                .extend(SystemContract::prague(chain_spec.deposit_contract().map(|c| c.address)));
        }

        insert_berachain_system_contracts(
            &chain_spec,
            system_contracts_eval_timestamp,
            &mut system_contracts,
        );

        let fork_id = chain_spec
            .fork_id(&Head { timestamp: fork_timestamp, number: u64::MAX, ..Default::default() })
            .hash
            .0
            .into();

        Some(EthForkConfig {
            activation_time: fork_timestamp,
            blob_schedule: chain_spec.blob_params_at_timestamp(fork_timestamp)?,
            chain_id: chain_spec.chain().id(),
            fork_id,
            precompiles,
            system_contracts,
        })
    }

    fn config(&self) -> Result<EthConfig, RethError> {
        let chain_spec = self.provider.chain_spec();
        let latest = self
            .provider
            .latest_header()?
            .ok_or_else(|| ProviderError::BestBlockNotFound)?
            .into_header();

        let current_precompiles = evm_to_precompiles_map(
            self.evm_config.evm_for_block(EmptyDB::default(), &latest).map_err(RethError::other)?,
        );

        let mut fork_timestamps =
            chain_spec.forks_iter().filter_map(|(_, cond)| cond.as_timestamp()).collect::<Vec<_>>();
        fork_timestamps.sort_unstable();
        fork_timestamps.dedup();

        let (current_fork_idx, current_fork_timestamp) = fork_timestamps
            .iter()
            .position(|ts| &latest.timestamp < ts)
            .and_then(|idx| idx.checked_sub(1))
            .or_else(|| fork_timestamps.len().checked_sub(1))
            .and_then(|idx| fork_timestamps.get(idx).map(|ts| (idx, *ts)))
            .ok_or_else(|| RethError::msg("no active timestamp fork found"))?;

        let current = self
            .build_fork_config_at(current_fork_timestamp, current_precompiles, latest.timestamp)
            .ok_or_else(|| RethError::msg("no fork config for current fork"))?;

        let mut config = EthConfig { current, next: None, last: None };

        if let Some(next_fork_timestamp) = fork_timestamps.get(current_fork_idx + 1).copied() {
            let fake_header = {
                let mut header = latest.clone();
                header.timestamp = next_fork_timestamp;
                header
            };
            let next_precompiles = evm_to_precompiles_map(
                self.evm_config
                    .evm_for_block(EmptyDB::default(), &fake_header)
                    .map_err(RethError::other)?,
            );

            config.next = self.build_fork_config_at(
                next_fork_timestamp,
                next_precompiles,
                next_fork_timestamp,
            );
        } else {
            // If there is no fork scheduled, there is no "last" or "final" fork scheduled.
            return Ok(config);
        }

        let last_fork_timestamp = fork_timestamps.last().copied().unwrap();
        let fake_header = {
            let mut header = latest;
            header.timestamp = last_fork_timestamp;
            header
        };
        let last_precompiles = evm_to_precompiles_map(
            self.evm_config
                .evm_for_block(EmptyDB::default(), &fake_header)
                .map_err(RethError::other)?,
        );

        config.last =
            self.build_fork_config_at(last_fork_timestamp, last_precompiles, last_fork_timestamp);

        Ok(config)
    }
}

impl<Provider> EthConfigApiServer for BerachainConfigHandler<Provider>
where
    Provider: ChainSpecProvider<ChainSpec = BerachainChainSpec>
        + BlockReaderIdExt<Header = BerachainHeader>
        + 'static,
{
    fn config(&self) -> RpcResult<EthConfig> {
        Ok(self.config().map_err(EthApiError::from)?)
    }
}

fn insert_berachain_system_contracts(
    chain_spec: &BerachainChainSpec,
    eval_timestamp: u64,
    system_contracts: &mut BTreeMap<SystemContract, Address>,
) {
    // BRIP-0004: PoL distributor is a Prague1 system contract, not part of Ethereum Prague.
    if let Some(address) = chain_spec.active_pol_distributor_at_timestamp(eval_timestamp) {
        system_contracts
            .insert(SystemContract::Other(POL_DISTRIBUTOR_SYSTEM_CONTRACT.into()), address);
    }
}

fn evm_to_precompiles_map(
    evm: impl Evm<Precompiles = PrecompilesMap>,
) -> BTreeMap<String, Address> {
    let precompiles = evm.precompiles();
    precompiles
        .addresses()
        .filter_map(|address| {
            Some((precompiles.get(address)?.precompile_id().name().to_string(), *address))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chainspec::BerachainChainSpecParser, hardforks::BerachainHardforks};
    use alloy_primitives::address;
    use reth_cli::chainspec::ChainSpecParser;

    const BERACHAIN_BEPOLIA: &str = "bepolia";

    #[test]
    fn test_insert_berachain_system_contracts_bepolia_boundary() {
        let chain_spec = BerachainChainSpecParser::parse(BERACHAIN_BEPOLIA).unwrap();
        let expected = address!("D2f19a79b026Fb636A7c300bF5947df113940761");

        let mut before_prague1 = BTreeMap::default();
        insert_berachain_system_contracts(&chain_spec, 1_754_495_999, &mut before_prague1);
        assert!(before_prague1.is_empty());

        let mut after_prague1 = BTreeMap::default();
        insert_berachain_system_contracts(&chain_spec, 1_754_496_000, &mut after_prague1);
        assert_eq!(
            after_prague1.get(&SystemContract::Other(POL_DISTRIBUTOR_SYSTEM_CONTRACT.into())),
            Some(&expected),
        );
    }

    #[test]
    fn test_pol_distributor_uses_head_timestamp_not_prague_fork_timestamp() {
        let chain_spec = BerachainChainSpecParser::parse(BERACHAIN_BEPOLIA).unwrap();
        let expected = address!("D2f19a79b026Fb636A7c300bF5947df113940761");
        let prague_fork_timestamp = 1_746_633_600;
        let post_prague1_head_timestamp = 1_755_000_000;

        assert!(!chain_spec.is_prague1_active_at_timestamp(prague_fork_timestamp));
        assert!(chain_spec.is_prague1_active_at_timestamp(post_prague1_head_timestamp));

        let mut at_prague_fork = BTreeMap::default();
        insert_berachain_system_contracts(&chain_spec, prague_fork_timestamp, &mut at_prague_fork);
        assert!(at_prague_fork.is_empty());

        let mut at_head = BTreeMap::default();
        insert_berachain_system_contracts(&chain_spec, post_prague1_head_timestamp, &mut at_head);
        assert_eq!(
            at_head.get(&SystemContract::Other(POL_DISTRIBUTOR_SYSTEM_CONTRACT.into())),
            Some(&expected),
        );
    }

    #[test]
    fn test_pol_distributor_system_contract_serde() {
        let contract = SystemContract::Other(POL_DISTRIBUTOR_SYSTEM_CONTRACT.into());
        let value = serde_json::to_value(contract).unwrap();
        assert_eq!(value, "POL_DISTRIBUTOR_ADDRESS");
    }
}
