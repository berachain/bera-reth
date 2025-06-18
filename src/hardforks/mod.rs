//! Berachain hardfork definitions for use alongside Ethereum hardforks

use reth::chainspec::{EthereumHardforks, ForkCondition, hardfork};

hardfork!(
    /// Berachain hardforks to be mixed with [`EthereumHardfork`]
    BerachainHardfork {
        /// Prague1 hardfork: Enforces 1 gwei minimum base fee
        Prague1,
    }
);

/// Trait for querying Berachain hardfork activation status
pub trait BerachainHardforks: EthereumHardforks {
    /// Returns activation condition for a Berachain hardfork
    fn berachain_fork_activation(&self, fork: BerachainHardfork) -> ForkCondition;

    /// Checks if Prague1 hardfork is active at given timestamp
    fn is_prague1_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.berachain_fork_activation(BerachainHardfork::Prague1).active_at_timestamp(timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth::chainspec::{EthereumHardfork, EthereumHardforks, ForkCondition};

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
    fn test_prague1_hardfork() {
        let fork = BerachainHardfork::Prague1;
        assert_eq!(format!("{fork:?}"), "Prague1");
    }

    #[test]
    fn test_hardforks_trait_implementation() {
        let hardforks = MockHardforks;

        // Test Prague1 activation at genesis (timestamp 0)
        let activation = hardforks.berachain_fork_activation(BerachainHardfork::Prague1);
        assert_eq!(activation, ForkCondition::Timestamp(0));

        // Test Prague1 active at timestamp using trait method
        assert!(hardforks.is_prague1_active_at_timestamp(0));
        assert!(hardforks.is_prague1_active_at_timestamp(100));
    }
}
