//! # Bera-Reth: A High-Performance Rust Execution Client for Berachain
//!
//! Bera-Reth is a custom execution client built on the Reth SDK, specifically designed
//! for the Berachain blockchain. It provides Ethereum-compatible execution while
//! implementing Berachain-specific features and optimizations.
//!
//! ## Key Components
//!
//! - **Chain Specification** ([`chainspec`]): Defines Berachain's network parameters, hardforks,
//!   and consensus rules
//! - **Genesis Configuration** ([`genesis`]): Handles initial blockchain state and
//!   Berachain-specific genesis parameters
//! - **Hardforks** ([`hardforks`]): Implements Berachain's custom hardforks alongside standard
//!   Ethereum upgrades
//! - **Node Implementation** ([`node`]): Provides the core node functionality with custom EVM
//!   configuration and execution logic
//!
//! ## Architecture
//!
//! Bera-Reth extends Reth's modular architecture to support:
//! - Custom hardfork activation (Prague1 with minimum base fee enforcement)
//! - Berachain-specific consensus integration via BeaconKit
//! - Enhanced EIP-1559 base fee mechanism
//! - Full Ethereum compatibility with Berachain extensions
//!
//! ## Example Usage
//!
//! ```no_run
//! use bera_reth::{
//!     chainspec::BerachainChainSpecParser,
//!     node::{BerachainNode, cli::Cli},
//! };
//! use clap::Parser;
//!
//! // Parse CLI arguments and launch node
//! let cli = Cli::<BerachainChainSpecParser>::parse();
//! // Node launching logic would follow...
//! ```

pub mod chainspec;
pub mod genesis;
pub mod hardforks;
pub mod node;
