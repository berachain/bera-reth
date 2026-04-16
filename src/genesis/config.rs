//! Berachain fork configuration types

use reth::revm::primitives::Address;
use serde::{Deserialize, Serialize};

/// Configuration for Prague1 hardfork activation
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prague1Config {
    /// Unix timestamp when Prague1 activates
    pub time: u64,
    /// Denominator for base fee change calculations (must be > 0)
    pub base_fee_change_denominator: u128,
    /// Minimum base fee in wei enforced after activation
    pub minimum_base_fee_wei: u64,
    /// PoL distributor contract address
    pub pol_distributor_address: Address,
}

/// Configuration for Prague2 hardfork activation
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prague2Config {
    /// Unix timestamp when Prague2 activates
    pub time: u64,
    /// Minimum base fee in wei enforced after activation
    pub minimum_base_fee_wei: u64,
}

/// Configuration for Prague3 hardfork activation
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prague3Config {
    /// Unix timestamp when Prague3 activates
    pub time: u64,
    /// List of addresses that are blocked from sending or receiving ERC20 transfers
    pub blocked_addresses: Vec<Address>,
    /// Rescue address where blocked addresses can send ERC20 tokens
    pub rescue_address: Address,
    /// BEX vault address to block InternalBalanceChanged events from
    pub bex_vault_address: Address,
}

/// Configuration for Prague4 hardfork activation
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prague4Config {
    /// Unix timestamp when Prague4 activates (ending Prague3 restrictions)
    pub time: u64,
}

/// Berachain-specific Osaka hardfork configuration (BRIP-0010)
///
/// Activates EthereumHardfork::Osaka on the EVM side, enabling:
/// - EIP-7951: P-256 (secp256r1) signature verification precompile at 0x100
/// - EIP-7939: CLZ (Count Leading Zeros) opcode
/// - EIP-7823/7883: MODEXP input bounds and gas repricing
/// - EIP-7934/7825: Block and transaction size caps
/// - Contract code size limit increase (24 KB -> 32 KB, initcode 48 KB -> 64 KB)
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsakaConfig {
    /// Unix timestamp when Osaka activates
    pub time: u64,
}
