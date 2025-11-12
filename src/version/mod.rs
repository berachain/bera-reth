//! Bera-Reth version configuration using Reth's configurable global versions

use reth_node_core::version::{RethCliVersionConsts, try_init_version_metadata};
use std::borrow::Cow;

#[derive(Debug)]
pub struct VersionInitError;

/// Initialize Bera-Reth version metadata using build.rs generated info
pub fn init_bera_version() -> Result<(), VersionInitError> {
    try_init_version_metadata(RethCliVersionConsts {
        name_client: Cow::Borrowed("Bera-Reth"),
        cargo_pkg_version: Cow::Borrowed(env!("CARGO_PKG_VERSION")),
        vergen_git_sha_long: Cow::Borrowed(env!("VERGEN_GIT_SHA")),
        vergen_git_sha: Cow::Borrowed(env!("VERGEN_GIT_SHA_SHORT")),
        vergen_build_timestamp: Cow::Borrowed(env!("VERGEN_BUILD_TIMESTAMP")),
        vergen_cargo_target_triple: Cow::Borrowed(env!("VERGEN_CARGO_TARGET_TRIPLE")),
        vergen_cargo_features: Cow::Borrowed(env!("VERGEN_CARGO_FEATURES")),
        short_version: Cow::Borrowed(env!("BERA_RETH_SHORT_VERSION")),
        long_version: Cow::Owned(format!(
            "{}\n{}\n{}\n{}\n{}",
            env!("BERA_RETH_LONG_VERSION_0"),
            env!("BERA_RETH_LONG_VERSION_1"),
            env!("BERA_RETH_LONG_VERSION_2"),
            env!("BERA_RETH_LONG_VERSION_3"),
            env!("BERA_RETH_LONG_VERSION_4"),
        )),
        build_profile_name: Cow::Borrowed(env!("BERA_RETH_BUILD_PROFILE")),
        p2p_client_version: Cow::Borrowed(env!("BERA_RETH_P2P_CLIENT_VERSION")),
        extra_data: Cow::Owned(format!(
            "bera-reth/v{}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        )),
    })
    .map_err(|_| VersionInitError)
}
