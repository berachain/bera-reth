//! Berachain hardfork definitions for use alongside Ethereum hardforks

use reth::chainspec::{EthereumHardforks, ForkCondition, hardfork};

hardfork!(
    /// Berachain hardforks to be mixed with [`EthereumHardfork`]
    BerachainHardfork {
        /// Prague1 hardfork: Introduces BRIP-0002 and BRIP-0004
        Prague1,
        /// Prague2 hardfork: Changes min base fee to 0
        Prague2,
        /// Prague3 hardfork: Blocks certain addresses from ERC20 transfers
        Prague3,
        /// Prague4 hardfork: Ends Prague3 restrictions
        Prague4,
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

    /// Checks if Prague2 hardfork is active at given timestamp
    fn is_prague2_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.berachain_fork_activation(BerachainHardfork::Prague2).active_at_timestamp(timestamp)
    }

    /// Checks if Prague3 hardfork is active at given timestamp
    /// Prague3 is active between its activation time and Prague4 activation
    fn is_prague3_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.berachain_fork_activation(BerachainHardfork::Prague3).active_at_timestamp(timestamp) &&
            !self.is_prague4_active_at_timestamp(timestamp)
    }

    /// Checks if Prague4 hardfork is active at given timestamp
    fn is_prague4_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.berachain_fork_activation(BerachainHardfork::Prague4).active_at_timestamp(timestamp)
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
                BerachainHardfork::Prague2 => ForkCondition::Timestamp(1000),
                BerachainHardfork::Prague3 => ForkCondition::Timestamp(2000),
                BerachainHardfork::Prague4 => ForkCondition::Timestamp(3000),
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
        assert!(hardforks.is_prague1_active_at_timestamp(0));
        assert!(hardforks.is_prague1_active_at_timestamp(100));

        // Test Prague2 activation and ordering
        let activation = hardforks.berachain_fork_activation(BerachainHardfork::Prague2);
        assert_eq!(activation, ForkCondition::Timestamp(1000));
        assert!(!hardforks.is_prague2_active_at_timestamp(999));
        assert!(hardforks.is_prague2_active_at_timestamp(1000));
        assert!(hardforks.is_prague2_active_at_timestamp(2000));

        // Test Prague3 activation and ordering
        let activation = hardforks.berachain_fork_activation(BerachainHardfork::Prague3);
        assert_eq!(activation, ForkCondition::Timestamp(2000));
        assert!(!hardforks.is_prague3_active_at_timestamp(1999));
        assert!(hardforks.is_prague3_active_at_timestamp(2000));
        assert!(hardforks.is_prague3_active_at_timestamp(2999)); // Active before Prague4
        assert!(!hardforks.is_prague3_active_at_timestamp(3000)); // Inactive after Prague4

        // Test Prague4 activation and effect on Prague3
        let activation = hardforks.berachain_fork_activation(BerachainHardfork::Prague4);
        assert_eq!(activation, ForkCondition::Timestamp(3000));
        assert!(!hardforks.is_prague4_active_at_timestamp(2999));
        assert!(hardforks.is_prague4_active_at_timestamp(3000));
        assert!(hardforks.is_prague4_active_at_timestamp(4000));
    }
}
