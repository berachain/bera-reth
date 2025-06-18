//! Tests for Berachain EVM executor functionality

use super::*;

#[test]
fn test_berachain_executor_builder_default() {
    let builder = BerachainExecutorBuilder::default();
    assert_eq!(format!("{:?}", builder), "BerachainExecutorBuilder");
}

#[test]
fn test_berachain_executor_builder_debug() {
    let builder = BerachainExecutorBuilder;
    let debug_str = format!("{:?}", builder);
    assert!(debug_str.contains("BerachainExecutorBuilder"));
}

#[test]
fn test_berachain_executor_builder_clone() {
    let builder = BerachainExecutorBuilder;
    let cloned = builder.clone();

    // Both should be the same as they're zero-sized types
    assert_eq!(format!("{:?}", builder), format!("{:?}", cloned));
}

#[test]
fn test_berachain_executor_builder_copy() {
    let builder = BerachainExecutorBuilder;
    let copied = builder; // Copy due to Copy trait

    // Both should be usable
    let _builder1 = builder;
    let _builder2 = copied;
}
