//! Berachain genesis configuration parsing and validation

use jsonrpsee_core::__reexports::serde_json;
use reth::{
    revm::primitives::{Address, address},
    rpc::types::serde_helpers::OtherFields,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors for Berachain genesis configuration parsing
#[derive(Debug, Error)]
pub enum BerachainConfigError {
    /// Invalid configuration format or values
    #[error("Invalid berachain configuration: {0}")]
    InvalidConfig(#[from] serde_json::Error),

    /// Base fee change denominator cannot be zero as it would cause division by zero
    #[error("Base fee change denominator cannot be zero")]
    InvalidDenominator,

    /// PoL distributor address is missing from Prague1 configuration
    #[error("PoL distributor address is required in Prague1 configuration but was not provided")]
    MissingPoLDistributorAddress,
}

/// Configuration for a Berachain hardfork activation
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainForkConfig {
    /// Unix timestamp when this hardfork activates
    pub time: u64,
    /// Denominator for base fee change calculations (must be > 0)
    pub base_fee_change_denominator: u128,
    /// Minimum base fee in wei enforced after activation
    pub minimum_base_fee_wei: u64,
    /// PoL distributor contract address
    pub pol_distributor_address: Address,
}

/// Complete Berachain genesis configuration from JSON "berachain" field
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainGenesisConfig {
    /// Configuration for the Prague1 hardfork, which introduces minimum base fee enforcement
    pub prague1: Option<BerachainForkConfig>,
}

impl Default for BerachainGenesisConfig {
    /// Default config with Prague1 activated immediately at genesis
    fn default() -> Self {
        Self {
            prague1: Some(BerachainForkConfig {
                time: 0,                             // Activate immediately at genesis
                base_fee_change_denominator: 48,     // Berachain standard value
                minimum_base_fee_wei: 1_000_000_000, // 1 gwei
                pol_distributor_address: address!("4200000000000000000000000000000000000042"),
            }),
        }
    }
}

impl BerachainGenesisConfig {
    /// Returns true if it's a berachain genesis
    pub fn is_berachain(&self) -> bool {
        self.prague1.is_some()
    }
}

impl TryFrom<&OtherFields> for BerachainGenesisConfig {
    type Error = BerachainConfigError;

    /// Parse BerachainGenesisConfig from genesis "berachain" field
    fn try_from(others: &OtherFields) -> Result<Self, Self::Error> {
        use tracing::info;

        match others.get_deserialized::<Self>("berachain") {
            Some(Ok(cfg)) => {
                // If prague1 is configured, validate it fully
                if let Some(prague1_config) = cfg.prague1 {
                    if prague1_config.base_fee_change_denominator == 0 {
                        return Err(BerachainConfigError::InvalidDenominator);
                    }
                    if prague1_config.pol_distributor_address.is_zero() {
                        return Err(BerachainConfigError::MissingPoLDistributorAddress);
                    }

                    info!(
                        "Loaded Berachain genesis configuration: Prague1 enabled at time={}, base_fee_denominator={}, min_base_fee={} gwei, pol_distributor={}",
                        prague1_config.time,
                        prague1_config.base_fee_change_denominator,
                        prague1_config.minimum_base_fee_wei / 1_000_000_000,
                        prague1_config.pol_distributor_address
                    );
                } else {
                    info!(
                        "Loaded Berachain genesis configuration: Prague1 not configured, defaulting to Ethereum behavior"
                    );
                }

                Ok(cfg)
            }
            Some(Err(e)) => Err(BerachainConfigError::InvalidConfig(e)),
            None => Ok(Self { prague1: None }),
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
        let cfg = BerachainGenesisConfig::try_from(&other_fields).expect("should succeed");

        // Should return a config that indicates it's not a berachain genesis
        assert_eq!(cfg.prague1, None);
        assert!(!cfg.is_berachain());
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
                "minimumBaseFeeWei": 1000000000,
                "polDistributorAddress": "0x4200000000000000000000000000000000000042"
            }
          }
        }
        "#;

        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let cfg = BerachainGenesisConfig::try_from(&other_fields)
            .expect("berachain field must deserialize");

        let prague1_config = cfg.prague1.expect("Prague1 should be configured");
        assert_eq!(prague1_config.time, 1620000000);
        assert_eq!(prague1_config.minimum_base_fee_wei, 1000000000);
        assert_eq!(prague1_config.base_fee_change_denominator, 48);
        assert_eq!(
            prague1_config.pol_distributor_address,
            address!("4200000000000000000000000000000000000042")
        );
    }

    #[test]
    fn test_genesis_config_berachain_present_no_prague1() {
        // Berachain field present but no prague1 -> should be valid with prague1 = None
        let json = r#"
        {
          "berachain": {}
        }
        "#;
        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let cfg = BerachainGenesisConfig::try_from(&other_fields).expect("should succeed");

        // Prague1 should not be configured
        assert_eq!(cfg.prague1, None);
        assert!(!cfg.is_berachain());
    }

    #[test]
    fn test_genesis_config_try_from_error_handling() {
        // Test that try_from returns errors instead of panicking
        let json = r#"
        {
          "berachain": {
            "prague1": {
                "time": 0,
                "baseFeeChangeDenominator": 0,
                "minimumBaseFeeWei": 1000000000,
                "polDistributorAddress": "0x4200000000000000000000000000000000000042"
            }
          }
        }
        "#;
        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let result = BerachainGenesisConfig::try_from(&other_fields);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Base fee change denominator cannot be zero")
        );
    }

    #[test]
    fn test_genesis_config_missing_pol_distributor_address() {
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

        let res = BerachainGenesisConfig::try_from(&other_fields);
        assert!(
            res.expect_err("must be an error")
                .to_string()
                .contains("missing field `polDistributorAddress`")
        );
    }

    #[test]
    fn test_genesis_config_zero_pol_distributor_address() {
        let json = r#"
        {
          "berachain": {
            "prague1": {
                "time": 1620000000,
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 1000000000,
                "polDistributorAddress": "0x0000000000000000000000000000000000000000"
            }
          }
        }
        "#;

        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let res = BerachainGenesisConfig::try_from(&other_fields);
        assert!(
            res.expect_err("must be an error")
                .to_string()
                .contains("PoL distributor address is required")
        );
    }
}
