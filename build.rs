#![allow(missing_docs)]

use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use vergen::{Build, Cargo, Emitter};
use vergen_git2::Git2;

const MAINNET_FIXTURE_PATH: &str = "tests/fixtures/mainnet-genesis.json";
const BEPOLIA_FIXTURE_PATH: &str = "tests/fixtures/bepolia-genesis.json";
const MAINNET_BAKED_FILENAME: &str = "mainnet-eth-genesis.json";
const BEPOLIA_BAKED_FILENAME: &str = "bepolia-eth-genesis.json";

fn main() -> Result<(), Box<dyn Error>> {
    let mut emitter = Emitter::default();

    let build_builder = Build::builder().build_timestamp(true).build();
    emitter.add_instructions(&build_builder)?;

    let cargo_builder = Cargo::builder().features(true).target_triple(true).build();
    emitter.add_instructions(&cargo_builder)?;

    let git_builder = Git2::builder().describe(false, true, None).dirty(true).sha(false).build();
    emitter.add_instructions(&git_builder)?;

    emitter.emit_and_set()?;

    let sha = env::var("VERGEN_GIT_SHA")?;
    let sha_short = &sha[0..7];

    let is_dirty = env::var("VERGEN_GIT_DIRTY")? == "true";
    let not_on_tag = env::var("VERGEN_GIT_DESCRIBE")?.ends_with(&format!("-g{sha_short}"));
    let version_suffix = if is_dirty || not_on_tag { "-dev" } else { "" };
    println!("cargo:rustc-env=BERA_RETH_VERSION_SUFFIX={version_suffix}");

    // Set short SHA
    println!("cargo:rustc-env=VERGEN_GIT_SHA_SHORT={}", &sha[..8]);

    // Set the build profile
    let out_dir = env::var("OUT_DIR").unwrap();
    bake_eth_genesis_specs(&out_dir)?;
    let profile = out_dir.rsplit(std::path::MAIN_SEPARATOR).nth(3).unwrap();
    println!("cargo:rustc-env=BERA_RETH_BUILD_PROFILE={profile}");

    // Set formatted version strings
    let pkg_version = env!("CARGO_PKG_VERSION");

    // Short version for bera-reth: 1.1.0-rc.0 (defa64b2)
    println!("cargo:rustc-env=BERA_RETH_SHORT_VERSION={pkg_version}{version_suffix} ({sha_short})");

    // Long version for bera-reth
    println!("cargo:rustc-env=BERA_RETH_LONG_VERSION_0=Version: {pkg_version}{version_suffix}");
    println!("cargo:rustc-env=BERA_RETH_LONG_VERSION_1=Commit SHA: {sha}");
    println!(
        "cargo:rustc-env=BERA_RETH_LONG_VERSION_2=Build Timestamp: {}",
        env::var("VERGEN_BUILD_TIMESTAMP")?
    );
    println!(
        "cargo:rustc-env=BERA_RETH_LONG_VERSION_3=Build Features: {}",
        env::var("VERGEN_CARGO_FEATURES")?
    );
    println!("cargo:rustc-env=BERA_RETH_LONG_VERSION_4=Build Profile: {profile}");

    // P2P client version: bera-reth/v1.1.0-rc.0-428a6dc2f/aarch64-apple-darwin
    println!(
        "cargo:rustc-env=BERA_RETH_P2P_CLIENT_VERSION={}",
        format_args!(
            "bera-reth/v{pkg_version}-{sha_short}/{}",
            env::var("VERGEN_CARGO_TARGET_TRIPLE")?
        )
    );

    Ok(())
}

fn bake_eth_genesis_specs(out_dir: &str) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={MAINNET_FIXTURE_PATH}");
    println!("cargo:rerun-if-changed={BEPOLIA_FIXTURE_PATH}");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let mainnet_out = Path::new(out_dir).join(MAINNET_BAKED_FILENAME);
    let bepolia_out = Path::new(out_dir).join(BEPOLIA_BAKED_FILENAME);

    copy_fixture_to_out_dir(&manifest_dir, MAINNET_FIXTURE_PATH, &mainnet_out)?;
    copy_fixture_to_out_dir(&manifest_dir, BEPOLIA_FIXTURE_PATH, &bepolia_out)?;

    Ok(())
}

fn copy_fixture_to_out_dir(
    manifest_dir: &str,
    fixture_relative_path: &str,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    let source = PathBuf::from(manifest_dir).join(fixture_relative_path);
    fs::copy(source, destination)?;
    Ok(())
}
