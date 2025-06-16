use jsonrpsee_core::__reexports::serde_json;
use reth::rpc::types::serde_helpers::OtherFields;
use serde::{Deserialize, Serialize, de::Error};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainForkConfig {
    pub time: u64,
    pub base_fee_change_denominator: u128,
    pub minimum_base_fee_wei: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BerachainGenesisConfig {
    pub prague1: BerachainForkConfig,
}

impl TryFrom<&OtherFields> for BerachainGenesisConfig {
    type Error = serde_json::Error;

    fn try_from(others: &OtherFields) -> Result<Self, Self::Error> {
        match others.get_deserialized::<Self>("berachain") {
            Some(Ok(cfg)) => Ok(cfg),
            Some(Err(e)) => Err(e), // propagate the real serde error inside
            None => Err(Error::missing_field("berachain")),
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
            res.expect_err("must be an error").to_string().contains("missing field `berachain`")
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
