//! Bera-Reth: Ethereum execution client for Berachain
//!
//! Built on Reth SDK with Ethereum compatibility plus Prague1 hardfork for minimum base fee.

pub mod chainspec;
pub mod consensus;
pub mod engine;
pub mod evm;
pub mod genesis;
pub mod hardforks;
pub mod node;
pub mod platform;
pub mod pool;
pub mod primitives;
pub mod rpc;
#[cfg(test)]
pub mod test_utils;
pub mod transaction;
pub mod version;
