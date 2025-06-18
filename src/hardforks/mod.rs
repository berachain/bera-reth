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
