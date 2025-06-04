use reth::chainspec::{hardfork, EthereumHardforks, ForkCondition};

hardfork!(
    /// The name of a berachain hardfork.
    ///
    /// When building a list of hardforks for a chain, it's still expected to mix with [`EthereumHardfork`].
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    BerachainHardfork {
        /// Berachain `Prague1` hardfork
        Prague1,
    }
);
pub trait BerachainHardforks: EthereumHardforks {
    fn berachain_fork_activation(&self, fork: BerachainHardfork) -> ForkCondition;
    fn is_prague1_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.berachain_fork_activation(BerachainHardfork::Prague1).active_at_timestamp(timestamp)
    }
}