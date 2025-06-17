//! # Berachain Hardfork Definitions
//!
//! This module defines custom hardforks specific to the Berachain network.
//! These hardforks are designed to work alongside standard Ethereum hardforks,
//! providing additional functionality and protocol upgrades specific to Berachain's
//! consensus and economic model.
//!
//! ## Hardfork Timeline
//!
//! Berachain hardforks are activated based on timestamp, allowing for coordinated
//! upgrades across the network. Each hardfork introduces specific protocol changes:
//!
//! - **Prague1**: Introduces minimum base fee enforcement and enhanced EIP-1559 parameters
//!
//! ## Usage
//!
//! These hardforks should be used in conjunction with Ethereum hardforks when
//! building a complete hardfork schedule for the Berachain network.

use reth::chainspec::{EthereumHardforks, ForkCondition, hardfork};

hardfork!(
    /// Berachain-specific hardfork definitions.
    ///
    /// These hardforks extend Ethereum's standard upgrade path with Berachain-specific
    /// protocol improvements. They are designed to be mixed with [`EthereumHardfork`]
    /// when constructing the complete hardfork schedule.
    ///
    /// # Hardforks
    ///
    /// * [`Prague1`] - Introduces minimum base fee enforcement and economic parameter changes
    BerachainHardfork {
        /// Prague1 hardfork: Minimum Base Fee Enforcement
        ///
        /// This hardfork introduces:
        /// - Minimum base fee of 1 gwei (1,000,000,000 wei)
        /// - Enhanced base fee calculation parameters
        /// - Economic incentive alignment for Berachain's PoL consensus
        ///
        /// Activated via timestamp-based fork condition.
        Prague1,
    }
);

/// Trait providing access to Berachain-specific hardfork activation conditions.
///
/// This trait extends [`EthereumHardforks`] to provide methods for querying
/// Berachain custom hardfork activation status. It should be implemented by
/// any chain specification that supports Berachain hardforks.
///
/// # Example
///
/// ```no_run
/// use bera_reth::hardforks::{BerachainHardfork, BerachainHardforks};
/// use reth::chainspec::ForkCondition;
///
/// fn check_prague1_active<T: BerachainHardforks>(chain: &T, timestamp: u64) -> bool {
///     chain.is_prague1_active_at_timestamp(timestamp)
/// }
/// ```
pub trait BerachainHardforks: EthereumHardforks {
    /// Returns the activation condition for a given Berachain hardfork.
    ///
    /// # Arguments
    ///
    /// * `fork` - The Berachain hardfork to query
    ///
    /// # Returns
    ///
    /// The [`ForkCondition`] that determines when this hardfork activates
    fn berachain_fork_activation(&self, fork: BerachainHardfork) -> ForkCondition;

    /// Checks if the Prague1 hardfork is active at a given timestamp.
    ///
    /// This is a convenience method that checks the Prague1 hardfork activation
    /// against the provided timestamp.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - Unix timestamp to check against
    ///
    /// # Returns
    ///
    /// `true` if Prague1 is active at the given timestamp, `false` otherwise
    fn is_prague1_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.berachain_fork_activation(BerachainHardfork::Prague1).active_at_timestamp(timestamp)
    }
}
