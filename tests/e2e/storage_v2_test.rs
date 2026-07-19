//! Storage V2 migration and V1 backward-compatibility tests.
//!
//! These drive the actual `bera-reth` binary the way an operator would:
//! `init` datadirs in each layout, migrate a V1 datadir in place with
//! `db migrate-v2`, and confirm the persisted layout via `db settings get`.

use std::{
    path::Path,
    process::{Command, Output},
};

fn genesis_path() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json")
}

fn run_bera_reth(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bera-reth"))
        .args(args)
        .output()
        .expect("failed to spawn bera-reth binary")
}

fn init_datadir(datadir: &Path, storage_v2: Option<bool>) {
    let datadir = datadir.to_str().unwrap();
    let mut args = vec!["init", "--chain", genesis_path(), "--datadir", datadir];
    let flag;
    if let Some(v2) = storage_v2 {
        flag = format!("--storage.v2={v2}");
        args.push(&flag);
    }
    let out = run_bera_reth(&args);
    assert!(
        out.status.success(),
        "init failed for {datadir}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn storage_settings(datadir: &Path) -> String {
    let out = run_bera_reth(&[
        "db",
        "--chain",
        genesis_path(),
        "--datadir",
        datadir.to_str().unwrap(),
        "settings",
        "get",
    ]);
    assert!(
        out.status.success(),
        "db settings get failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn migrate_v2(datadir: &Path) -> Output {
    run_bera_reth(&[
        "db",
        "--chain",
        genesis_path(),
        "--datadir",
        datadir.to_str().unwrap(),
        "migrate-v2",
    ])
}

/// Fresh datadirs must default to the V2 (hot/cold) storage layout.
#[test]
fn test_fresh_datadir_defaults_to_storage_v2() {
    let dir = tempfile::tempdir().unwrap();
    init_datadir(dir.path(), None);

    let settings = storage_settings(dir.path());
    assert!(
        settings.contains("storage_v2: true"),
        "fresh datadir should default to V2, got: {settings}"
    );
}

/// `--storage.v2=false` must still create a V1 datadir (escape hatch while
/// upstream keeps V1 support).
#[test]
fn test_storage_v2_flag_opts_new_datadir_into_v1() {
    let dir = tempfile::tempdir().unwrap();
    init_datadir(dir.path(), Some(false));

    let settings = storage_settings(dir.path());
    assert!(
        settings.contains("storage_v2: false"),
        "--storage.v2=false datadir should persist V1 settings, got: {settings}"
    );
}

/// A V1 datadir must remain readable by the v2-based binary: the persisted
/// layout wins over the flag default, and re-running genesis init against it
/// must recognize the existing genesis instead of failing.
#[test]
fn test_v1_datadir_remains_readable() {
    let dir = tempfile::tempdir().unwrap();
    init_datadir(dir.path(), Some(false));

    // Re-init without the flag: the persisted V1 settings must win and the
    // stored genesis (custom BerachainHeader codec) must decode and match.
    init_datadir(dir.path(), None);

    let settings = storage_settings(dir.path());
    assert!(
        settings.contains("storage_v2: false"),
        "existing V1 datadir must keep its persisted layout, got: {settings}"
    );
}

/// `db migrate-v2` must convert a V1 datadir in place and leave it readable.
#[test]
fn test_migrate_v2_converts_v1_datadir_in_place() {
    let dir = tempfile::tempdir().unwrap();
    init_datadir(dir.path(), Some(false));
    assert!(storage_settings(dir.path()).contains("storage_v2: false"));

    let out = migrate_v2(dir.path());
    assert!(out.status.success(), "migrate-v2 failed: {}", String::from_utf8_lossy(&out.stderr));

    let settings = storage_settings(dir.path());
    assert!(
        settings.contains("storage_v2: true"),
        "migrate-v2 should flip settings to V2, got: {settings}"
    );

    // The migrated datadir must still open and match the stored genesis.
    init_datadir(dir.path(), None);
}

/// Running `db migrate-v2` against an already-V2 datadir must be a no-op.
#[test]
fn test_migrate_v2_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    init_datadir(dir.path(), None);

    let out = migrate_v2(dir.path());
    assert!(
        out.status.success(),
        "migrate-v2 on a V2 datadir should be a successful no-op: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(storage_settings(dir.path()).contains("storage_v2: true"));
}
