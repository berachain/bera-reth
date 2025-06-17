//! # Berachain Genesis Configuration
//!
//! This module handles parsing and validation of Berachain-specific genesis parameters.
//! It extends the standard Ethereum genesis format with custom fields required for
//! Berachain's hardforks and consensus mechanisms.
//!
//! ## Key Types
//!
//! - [`BerachainGenesisConfig`]: Main configuration structure containing all Berachain-specific
//!   genesis parameters
//! - [`BerachainForkConfig`]: Configuration for individual Berachain hardforks
//! - [`BerachainConfigError`]: Comprehensive error handling for configuration parsing
//!
//! ## Example Genesis Format
//!
//! ```json
//! {
//!   "berachain": {
//!     "prague1": {
//!       "time": 1620000000,
//!       "baseFeeChangeDenominator": 48,
//!       "minimumBaseFeeWei": 1000000000
//!     }
//!   }
//! }
//! ```

use jsonrpsee_core::__reexports::serde_json;
use reth::rpc::types::serde_helpers::OtherFields;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Comprehensive error types for Berachain genesis configuration parsing and validation.
#[derive(Debug, Error)]
pub enum BerachainConfigError {
    /// The required 'berachain' field is missing from the genesis configuration
    #[error("Missing required 'berachain' field in genesis configuration")]
    MissingBerachainField,

    /// Invalid configuration format or values
    #[error("Invalid berachain configuration: {0}")]
    InvalidConfig(#[from] serde_json::Error),

    /// Base fee change denominator cannot be zero as it would cause division by zero
    #[error("Base fee change denominator cannot be zero")]
    InvalidDenominator,

    /// Fork activation time is invalid (e.g., in the past for future forks)
    #[error("Invalid fork activation time: {0}")]
    InvalidActivationTime(u64),
}

/// Configuration parameters for a Berachain hardfork.
///
/// This structure defines the activation time and economic parameters
/// that take effect when a Berachain hardfork activates.
///
/// # Fields
///
/// * `time` - Unix timestamp when this hardfork activates
/// * `base_fee_change_denominator` - Denominator used in EIP-1559 base fee calculations
/// * `minimum_base_fee_wei` - Minimum base fee enforced after activation (in wei)
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainForkConfig {
    /// Unix timestamp when this hardfork activates
    pub time: u64,
    /// Denominator for base fee change calculations (must be > 0)
    pub base_fee_change_denominator: u128,
    /// Minimum base fee in wei enforced after activation
    pub minimum_base_fee_wei: u64,
}

/// Complete Berachain genesis configuration containing all custom hardfork parameters.
///
/// This structure is parsed from the "berachain" field in the genesis JSON file
/// and contains configuration for all Berachain-specific hardforks.
///
/// # Example
///
/// ```
/// use bera_reth::genesis::{BerachainForkConfig, BerachainGenesisConfig};
///
/// let config = BerachainGenesisConfig {
///     prague1: BerachainForkConfig {
///         time: 1620000000,
///         base_fee_change_denominator: 48,
///         minimum_base_fee_wei: 1_000_000_000, // 1 gwei
///     },
/// };
/// ```
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainGenesisConfig {
    /// Configuration for the Prague1 hardfork, which introduces minimum base fee enforcement
    pub prague1: BerachainForkConfig,
}

impl Default for BerachainGenesisConfig {
    /// Creates a default Berachain genesis configuration.
    ///
    /// This provides sensible defaults for development and testing:
    /// - Prague1 activated far in the future (timestamp: u64::MAX)
    /// - Berachain standard base fee change denominator (48)
    /// - Minimum base fee of 1 gwei
    fn default() -> Self {
        Self {
            prague1: BerachainForkConfig {
                time: u64::MAX,                      // Far future - effectively disabled
                base_fee_change_denominator: 48,     // Berachain standard value
                minimum_base_fee_wei: 1_000_000_000, // 1 gwei
            },
        }
    }
}

impl BerachainForkConfig {
    /// Creates a new validated BerachainForkConfig.
    ///
    /// # Arguments
    ///
    /// * `time` - Unix timestamp for hardfork activation
    /// * `base_fee_change_denominator` - Must be greater than 0
    /// * `minimum_base_fee_wei` - Minimum base fee in wei
    ///
    /// # Errors
    ///
    /// Returns [`BerachainConfigError::InvalidDenominator`] if denominator is 0.
    pub fn new(
        time: u64,
        base_fee_change_denominator: u128,
        minimum_base_fee_wei: u64,
    ) -> Result<Self, BerachainConfigError> {
        if base_fee_change_denominator == 0 {
            return Err(BerachainConfigError::InvalidDenominator);
        }
        Ok(Self { time, base_fee_change_denominator, minimum_base_fee_wei })
    }
}

impl TryFrom<&OtherFields> for BerachainGenesisConfig {
    type Error = BerachainConfigError;

    /// Attempts to parse BerachainGenesisConfig from genesis file's "other" fields.
    ///
    /// This method looks for a "berachain" field in the genesis configuration
    /// and deserializes it into a BerachainGenesisConfig.
    ///
    /// # Errors
    ///
    /// * [`BerachainConfigError::MissingBerachainField`] - No "berachain" field found
    /// * [`BerachainConfigError::InvalidConfig`] - Invalid configuration format
    fn try_from(others: &OtherFields) -> Result<Self, Self::Error> {
        match others.get_deserialized::<Self>("berachain") {
            Some(Ok(cfg)) => {
                // Validate the parsed configuration
                if cfg.prague1.base_fee_change_denominator == 0 {
                    return Err(BerachainConfigError::InvalidDenominator);
                }
                Ok(cfg)
            }
            Some(Err(e)) => Err(BerachainConfigError::InvalidConfig(e)),
            None => Err(BerachainConfigError::MissingBerachainField),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonrpsee_core::__reexports::serde_json::Value;
    use reth::rpc::types::serde_helpers::OtherFields;

    #[test]
    fn test_genesis_config_missing_berachain_field() {
        let json = r#"
        {
        }
        "#;

        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");
        let res = BerachainGenesisConfig::try_from(&other_fields);
        assert!(
            res.expect_err("must be an error")
                .to_string()
                .contains("Missing required 'berachain' field")
        );
    }

    #[test]
    fn test_genesis_config_missing_time_field() {
        let json = r#"
        {
          "berachain": {
            "prague1": {
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 1000000000
            }
          }
        }
        "#;

        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let res = BerachainGenesisConfig::try_from(&other_fields);
        assert!(res.expect_err("must be an error").to_string().contains("missing field `time`"));
    }

    #[test]
    fn test_genesis_config_valid_genesis() {
        let json = r#"
        {
          "berachain": {
            "prague1": {
                "time": 1620000000,
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 1000000000
            }
          }
        }
        "#;

        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let cfg = BerachainGenesisConfig::try_from(&other_fields)
            .expect("berachain field must deserialize");

        assert_eq!(cfg.prague1.time, 1620000000);
        assert_eq!(cfg.prague1.minimum_base_fee_wei, 1000000000);
        assert_eq!(cfg.prague1.base_fee_change_denominator, 48);
    }
}
