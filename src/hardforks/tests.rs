//! Tests for Berachain hardforks functionality

use super::*;
use reth::chainspec::{EthereumHardfork, EthereumHardforks, ForkCondition};
use reth_chainspec::Hardfork;

struct MockHardforks;

impl EthereumHardforks for MockHardforks {
    fn ethereum_fork_activation(&self, _fork: EthereumHardfork) -> ForkCondition {
        ForkCondition::Block(0)
    }
}

impl BerachainHardforks for MockHardforks {
    fn berachain_fork_activation(&self, fork: BerachainHardfork) -> ForkCondition {
        match fork {
            BerachainHardfork::Prague1 => ForkCondition::Timestamp(0),
        }
    }
}

#[test]
fn test_berachain_hardfork_prague1_exists() {
    let fork = BerachainHardfork::Prague1;
    assert_eq!(format!("{:?}", fork), "Prague1");
}

#[test]
fn test_berachain_hardforks_trait_implementation() {
    let hardforks = MockHardforks;

    // Test Prague1 activation
    let activation = hardforks.berachain_fork_activation(BerachainHardfork::Prague1);
    assert_eq!(activation, ForkCondition::Timestamp(0));

    // Test Prague1 active at timestamp
    assert!(hardforks.is_prague1_active_at_timestamp(0));
    assert!(hardforks.is_prague1_active_at_timestamp(100));
}

#[test]
fn test_prague1_activation_before_timestamp() {
    let hardforks = MockHardforks;

    // Prague1 is active at genesis (timestamp 0)
    assert!(hardforks.is_prague1_active_at_timestamp(0));

    // And should remain active after
    assert!(hardforks.is_prague1_active_at_timestamp(1000));
}

#[test]
fn test_berachain_hardfork_ordering() {
    // Test that Prague1 can be converted to boxed hardfork
    let fork = BerachainHardfork::Prague1;
    let _boxed = fork.boxed();
}
