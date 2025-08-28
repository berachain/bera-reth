//! Compact test vectors for Berachain custom types
//!
//! This module provides functions to read and validate Berachain-specific compact test vectors.

use crate::primitives::BerachainHeader;
use eyre::Result;
use proptest::test_runner::TestRunner;
use reth_cli_commands::{
    compact_types,
    test_vectors::compact::{
        generate_vector, generate_vectors_with, read_vector, read_vectors_with,
    },
};

/// Generates test vectors for both reth standard types and Berachain extensions
pub fn generate_berachain_vectors() -> Result<()> {
    println!("Generating test vectors for berachain types...");

    generate_vectors_with(GENERATE_VECTORS)?;
    Ok(())
}

/// Reads and validates test vectors for BerachainHeader using reth's infrastructure
pub fn read_berachain_vectors() -> Result<()> {
    read_vectors_with(READ_VECTORS)?;
    Ok(())
}

compact_types!(
    regular: [BerachainHeader],
    identifier: []
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_berachain_header_test_vectors() {
        // Test that we can generate and read back BerachainHeader
        // let result = generate_berachain_vectors();
        // assert!(result.is_ok(), "Failed to generate BerachainHeader test vectors: {:?}", result);
        // generate_berachain_vectors().unwrap();

        read_berachain_vectors().unwrap();
    }
}
