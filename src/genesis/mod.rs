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
    pub prague1: BerachainForkConfig,
}

/// Default PoL contract address
fn default_pol_contract_address() -> Address {
    address!("4200000000000000000000000000000000000042")
}

impl Default for BerachainGenesisConfig {
    /// Default config with Prague1 activated immediately at genesis
    fn default() -> Self {
        Self {
            prague1: BerachainForkConfig {
                time: 0,                             // Activate immediately at genesis
                base_fee_change_denominator: 48,     // Berachain standard value
                minimum_base_fee_wei: 1_000_000_000, // 1 gwei
                pol_distributor_address: default_pol_contract_address(),
            },
        }
    }
}

impl BerachainForkConfig {
    /// Creates validated config. Returns error if denominator is 0.
    pub fn new(
        time: u64,
        base_fee_change_denominator: u128,
        minimum_base_fee_wei: u64,
        pol_distributor_address: Address,
    ) -> Result<Self, BerachainConfigError> {
        if base_fee_change_denominator == 0 {
            return Err(BerachainConfigError::InvalidDenominator);
        }
        Ok(Self {
            time,
            base_fee_change_denominator,
            minimum_base_fee_wei,
            pol_distributor_address,
        })
    }
}

impl TryFrom<&OtherFields> for BerachainGenesisConfig {
    type Error = BerachainConfigError;

    /// Parse BerachainGenesisConfig from genesis "berachain" field
    fn try_from(others: &OtherFields) -> Result<Self, Self::Error> {
        use tracing::info;

        match others.get_deserialized::<Self>("berachain") {
            Some(Ok(cfg)) => {
                // Validate the parsed configuration
                if cfg.prague1.base_fee_change_denominator == 0 {
                    return Err(BerachainConfigError::InvalidDenominator);
                }
                if cfg.prague1.pol_distributor_address.is_zero() {
                    return Err(BerachainConfigError::MissingPoLDistributorAddress);
                }

                info!(
                    "Loaded Berachain genesis configuration: Prague1 time={}, base_fee_denominator={}, min_base_fee={} gwei, pol_distributor={}",
                    cfg.prague1.time,
                    cfg.prague1.base_fee_change_denominator,
                    cfg.prague1.minimum_base_fee_wei / 1_000_000_000,
                    cfg.prague1.pol_distributor_address
                );

                Ok(cfg)
            }
            Some(Err(e)) => Err(BerachainConfigError::InvalidConfig(e)),
            None => {
                info!("No berachain configuration found in genesis, using defaults");
                Err(BerachainConfigError::MissingBerachainField)
            }
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

        assert_eq!(cfg.prague1.time, 1620000000);
        assert_eq!(cfg.prague1.minimum_base_fee_wei, 1000000000);
        assert_eq!(cfg.prague1.base_fee_change_denominator, 48);
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
