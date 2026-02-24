//! Berachain genesis configuration parsing and validation

pub mod config;

use jsonrpsee_core::__reexports::serde_json;
use reth::{revm::primitives::address, rpc::types::serde_helpers::OtherFields};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use config::{Prague1Config, Prague2Config, Prague3Config, Prague4Config};

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

/// Complete Berachain genesis configuration from JSON "berachain" field
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainGenesisConfig {
    /// Configuration for the Prague1 hardfork, which introduces minimum base fee enforcement
    pub prague1: Option<Prague1Config>,
    /// Configuration for the Prague2 hardfork, which reverts base fee to 0
    pub prague2: Option<Prague2Config>,
    /// Configuration for the Prague3 hardfork, which blocks events from specific token contracts
    pub prague3: Option<Prague3Config>,
    /// Configuration for the Prague4 hardfork, which ends Prague3 restrictions
    pub prague4: Option<Prague4Config>,
}

impl Default for BerachainGenesisConfig {
    /// Default config with Prague1 and Prague2 activated
    fn default() -> Self {
        Self {
            prague1: Some(Prague1Config {
                time: 0,                             // Activate immediately at genesis
                base_fee_change_denominator: 48,     // Berachain standard value
                minimum_base_fee_wei: 1_000_000_000, // 1 gwei
                pol_distributor_address: address!("4200000000000000000000000000000000000042"),
            }),
            prague2: Some(Prague2Config {
                time: 0,                 // Activate immediately at genesis
                minimum_base_fee_wei: 0, // 0 wei
            }),
            prague3: None, // Not activated by default
            prague4: None, // Not activated by default
        }
    }
}

impl BerachainGenesisConfig {
    /// Returns true if it's a berachain genesis (both Prague1 and Prague2 configured)
    pub fn is_berachain(&self) -> bool {
        self.prague1.is_some() && self.prague2.is_some()
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
                }

                // Validate that both Prague1 and Prague2 are configured together
                match (cfg.prague1, cfg.prague2) {
                    (Some(prague1_config), Some(prague2_config)) => {
                        // Both configured - validate Prague2 comes at or after Prague1
                        if prague2_config.time < prague1_config.time {
                            return Err(BerachainConfigError::InvalidConfig(serde_json::Error::io(
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "Prague2 hardfork must activate at or after Prague1 hardfork",
                                ),
                            )));
                        }
                    }
                    _ => {
                        return Err(BerachainConfigError::InvalidConfig(serde_json::Error::io(
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Berachain networks require both Prague1 and Prague2 hardforks to be configured",
                            ),
                        )));
                    }
                }

                Ok(cfg)
            }
            Some(Err(e)) => Err(BerachainConfigError::InvalidConfig(e)),
            None => Ok(Self { prague1: None, prague2: None, prague3: None, prague4: None }),
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
        assert_eq!(cfg.prague2, None);
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
            },
            "prague2": {
                "time": 1720000000,
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 0,
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

        let prague2_config = cfg.prague2.expect("Prague2 should be configured");
        assert_eq!(prague2_config.time, 1720000000);
        assert_eq!(prague2_config.minimum_base_fee_wei, 0);

        assert!(cfg.is_berachain());
    }

    #[test]
    fn test_genesis_config_berachain_present_no_prague1() {
        // Berachain field present but no prague1 -> should fail since both are required
        let json = r#"
        {
          "berachain": {}
        }
        "#;
        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let result = BerachainGenesisConfig::try_from(&other_fields);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(
            "Berachain networks require both Prague1 and Prague2 hardforks to be configured"
        ));
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

    #[test]
    fn test_genesis_config_prague2_without_prague1_fails() {
        let json = r#"
        {
          "berachain": {
            "prague2": {
                "time": 2000000000,
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 0,
                "polDistributorAddress": "0x4200000000000000000000000000000000000042"
            }
          }
        }
        "#;

        let v: Value = serde_json::from_str(json).unwrap();
        let other_fields = OtherFields::try_from(v).expect("must be a valid genesis config");

        let result = BerachainGenesisConfig::try_from(&other_fields);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(
            "Berachain networks require both Prague1 and Prague2 hardforks to be configured"
        ));
    }

    #[test]
    fn test_genesis_config_prague1_without_prague2_fails() {
        let json = r#"
        {
          "berachain": {
            "prague1": {
                "time": 1000000000,
                "baseFeeChangeDenominator": 48,
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
        assert!(result.unwrap_err().to_string().contains(
            "Berachain networks require both Prague1 and Prague2 hardforks to be configured"
        ));
    }

    #[test]
    fn test_genesis_config_prague2_before_prague1_fails() {
        let json = r#"
        {
          "berachain": {
            "prague1": {
                "time": 2000000000,
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 1000000000,
                "polDistributorAddress": "0x4200000000000000000000000000000000000042"
            },
            "prague2": {
                "time": 1000000000,
                "baseFeeChangeDenominator": 48,
                "minimumBaseFeeWei": 0,
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
            result
                .unwrap_err()
                .to_string()
                .contains("Prague2 hardfork must activate at or after Prague1 hardfork")
        );
    }
}
