//! Storage V2 migration and V1 backward-compatibility tests.
//!
//! These drive the actual `bera-reth` binary the way an operator would:
//! `init` datadirs in each layout, seed a V1 datadir with real blocks via
//! `import`, migrate it in place with `db migrate-v2`, and confirm both the
//! persisted layout (`db settings get`) and that the migrated data remains
//! queryable (`db get`, `db state`).

use crate::e2e::{
    POL_DISTRIBUTOR_ADDRESS, berachain_payload_attributes_generator, setup_test_boilerplate,
    test_signer,
};
use alloy_primitives::{Address, B256};
use alloy_rlp::Encodable;
use bera_reth::node::BerachainNode;
use reth_chainspec::EthChainSpec;
use reth_e2e_test_utils::{node::NodeTestContext, transaction::TransactionTestContext};
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use std::{
    path::Path,
    process::{Command, Output},
    sync::Arc,
};

const SLOT_ZERO: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

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

/// Runs a `bera-reth db <args>` subcommand and returns its trimmed stdout.
///
/// Logs are silenced (`-q`) so stdout carries only the decoded value that
/// `db get` and `db state` print; nothing is printed for a missing key, so
/// an empty return value signals a miss.
fn db_stdout(datadir: &Path, args: &[&str]) -> String {
    let mut full_args =
        vec!["db", "-q", "--chain", genesis_path(), "--datadir", datadir.to_str().unwrap()];
    full_args.extend_from_slice(args);
    let out = run_bera_reth(&full_args);
    assert!(out.status.success(), "db {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("expected JSON output, got {stdout:?}: {err}"))
}

/// Parses a history-index shard (`IntegerList` serializes as a JSON array of
/// block numbers).
fn shard_blocks(stdout: &str) -> Vec<u64> {
    serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("expected a JSON block list, got {stdout:?}: {err}"))
}

/// Chain data produced by [`build_seed_chain`], needed for migration asserts.
struct SeedChain {
    /// Blocks 1..=3, RLP-encoded back-to-back for `bera-reth import`.
    blocks_rlp: Vec<u8>,
    /// EOA that sent the transfers in blocks 1 and 2.
    sender: Address,
    /// Hash of the transfer in block 1 (global transaction number 1).
    transfer_in_block1: B256,
    /// Hash of the transfer in block 2 (global transaction number 3).
    transfer_in_block2: B256,
}

/// Mines three blocks on an ephemeral in-process node and returns them
/// RLP-encoded, ready for `bera-reth import` into a fresh datadir built from
/// the same genesis fixture.
///
/// Every block carries the PoL system transaction, which increments storage
/// slot 0 of the PoL distributor — so every block produces receipts, account
/// changesets, storage changesets, history-index entries, and tx-lookup
/// entries. Blocks 1 and 2 additionally carry an EIP-1559 transfer.
async fn build_seed_chain() -> eyre::Result<SeedChain> {
    let (runtime, chain_spec) = setup_test_boilerplate().await?;

    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(runtime.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes_generator).await?;
    let signer = test_signer()?;
    let sender = signer.address();
    let chain_id = chain_spec.chain_id();

    let mut blocks_rlp = Vec::new();
    let mut transfer_hashes = Vec::new();

    // Blocks 1 and 2: PoL system tx + one EIP-1559 transfer each.
    for nonce in 0..2 {
        let tx =
            TransactionTestContext::transfer_tx_bytes_with_nonce(chain_id, signer.clone(), nonce)
                .await;
        transfer_hashes.push(ctx.rpc.inject_tx(tx).await?);
        let payload = ctx.advance_block().await?;
        Arc::unwrap_or_clone(payload.block).into_block().encode(&mut blocks_rlp);
    }

    // Block 3: PoL system tx only.
    let payload = ctx.advance_block().await?;
    Arc::unwrap_or_clone(payload.block).into_block().encode(&mut blocks_rlp);

    Ok(SeedChain {
        blocks_rlp,
        sender,
        transfer_in_block1: transfer_hashes[0],
        transfer_in_block2: transfer_hashes[1],
    })
}

/// Writes `blocks_rlp` to a temporary chain file and imports it into
/// `datadir` through the full pipeline (senders, execution, hashing, merkle,
/// history indexing, tx lookup), populating every table migrate-v2 touches.
fn import_blocks(datadir: &Path, blocks_rlp: &[u8]) -> eyre::Result<()> {
    let rlp_dir = tempfile::tempdir()?;
    let rlp_path = rlp_dir.path().join("seed-blocks.rlp");
    std::fs::write(&rlp_path, blocks_rlp)?;
    let out = run_bera_reth(&[
        "import",
        "--chain",
        genesis_path(),
        "--datadir",
        datadir.to_str().unwrap(),
        "--fail-on-invalid-block",
        rlp_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "import failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(())
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

/// `db migrate-v2` on a datadir holding real chain data must move the
/// populated receipt, changeset, history-index, and transaction-lookup
/// tables into the V2 backends (static files + RocksDB) and leave every one
/// of them queryable afterwards.
#[tokio::test]
async fn test_migrate_v2_preserves_seeded_chain_data() -> eyre::Result<()> {
    let seed = build_seed_chain().await?;

    // Seed a V1 datadir: `init` it, then `import` the mined blocks through
    // the full pipeline, populating every table that migrate-v2 moves.
    let dir = tempfile::tempdir()?;
    init_datadir(dir.path(), Some(false));
    assert!(storage_settings(dir.path()).contains("storage_v2: false"));

    import_blocks(dir.path(), &seed.blocks_rlp)?;

    let tx1 = seed.transfer_in_block1.to_string();
    let tx2 = seed.transfer_in_block2.to_string();
    let sender = seed.sender.to_string();
    let sender_shard_key =
        format!(r#"{{"key":"{sender}","highest_block_number":18446744073709551615}}"#);

    // The V1 layout keeps changesets, history indices, and tx lookups in
    // MDBX; receipts of an unpruned V1 node already live in static files.
    // The PoL system tx is the first transaction of every block, so the
    // transfers are global transactions 1 (block 1) and 3 (block 2).
    assert_eq!(db_stdout(dir.path(), &["get", "mdbx", "TransactionHashNumbers", &tx1]), "1");
    assert_eq!(db_stdout(dir.path(), &["get", "mdbx", "TransactionHashNumbers", &tx2]), "3");
    parse_json(&db_stdout(dir.path(), &["get", "static-file", "receipts", "1"]));
    assert_eq!(
        shard_blocks(&db_stdout(
            dir.path(),
            &["get", "mdbx", "AccountsHistory", &sender_shard_key]
        )),
        vec![0, 1, 2],
        "sender history must cover the genesis alloc and both transfers"
    );

    let out = migrate_v2(dir.path());
    assert!(out.status.success(), "migrate-v2 failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(storage_settings(dir.path()).contains("storage_v2: true"));

    // Transaction lookups moved into RocksDB and out of MDBX.
    assert_eq!(db_stdout(dir.path(), &["get", "rocksdb", "transaction-hash-numbers", &tx1]), "1");
    assert_eq!(db_stdout(dir.path(), &["get", "rocksdb", "transaction-hash-numbers", &tx2]), "3");
    assert_eq!(db_stdout(dir.path(), &["get", "mdbx", "TransactionHashNumbers", &tx1]), "");

    // History indices moved into RocksDB and out of MDBX.
    assert_eq!(
        shard_blocks(&db_stdout(dir.path(), &["get", "rocksdb", "accounts-history", &sender])),
        vec![0, 1, 2],
    );
    assert_eq!(db_stdout(dir.path(), &["get", "mdbx", "AccountsHistory", &sender_shard_key]), "");
    assert_eq!(
        shard_blocks(&db_stdout(
            dir.path(),
            &[
                "get",
                "rocksdb",
                "storages-history",
                POL_DISTRIBUTOR_ADDRESS,
                "--storage-key",
                SLOT_ZERO,
            ]
        )),
        vec![1, 2, 3],
        "the PoL system tx increments distributor slot 0 in every block"
    );

    // Receipts must remain readable from static files: the EIP-1559 transfer
    // (tx 1) and the PoL system receipt of block 3 (tx 4, Berachain-specific
    // tx type).
    for tx_num in ["1", "4"] {
        let receipt =
            parse_json(&db_stdout(dir.path(), &["get", "static-file", "receipts", tx_num]));
        assert!(receipt["logs"].is_array(), "receipt {tx_num} must decode, got: {receipt}");
    }

    // Changesets moved into static files.
    let account_changes =
        db_stdout(dir.path(), &["get", "static-file", "account-change-sets", "1"]).to_lowercase();
    assert!(
        account_changes.contains(&sender.to_lowercase()),
        "block 1 account changeset must contain the transfer sender, got: {account_changes}"
    );
    let storage_changes = db_stdout(
        dir.path(),
        &[
            "get",
            "static-file",
            "storage-change-sets",
            r#"[3,"0x0000000000000000000000000000000000000000"]"#,
        ],
    )
    .to_lowercase();
    assert!(
        storage_changes.contains(POL_DISTRIBUTOR_ADDRESS),
        "block 3 storage changeset must contain the PoL distributor, got: {storage_changes}"
    );

    // Historical state queries resolve through the RocksDB history indices
    // plus the static-file changesets — the read path RPC uses after the
    // migration. Distributor slot 0 counts PoL distributions: 2 after block 2.
    let state = parse_json(&db_stdout(
        dir.path(),
        &["state", POL_DISTRIBUTOR_ADDRESS, "--block", "2", "--format", "json"],
    ));
    assert_eq!(state["storage"][0]["key"], SLOT_ZERO);
    assert_eq!(
        state["storage"][0]["value"],
        "0x0000000000000000000000000000000000000000000000000000000000000002"
    );

    // The sender's first transfer landed in block 1, so its nonce at block 1
    // must already be 1.
    let state =
        parse_json(&db_stdout(dir.path(), &["state", &sender, "--block", "1", "--format", "json"]));
    assert_eq!(state["account"]["nonce"], 1);

    // The migrated datadir must still open and match the stored genesis.
    init_datadir(dir.path(), None);
    Ok(())
}

/// An unpruned V1 datadir already keeps receipts in static files, but a V1
/// datadir with receipt pruning configured writes them to MDBX instead — the
/// one layout where `db migrate-v2` must actually copy receipts into static
/// files rather than taking its "already in static files" skip path.
#[tokio::test]
async fn test_migrate_v2_moves_pruned_receipts_to_static_files() -> eyre::Result<()> {
    let seed = build_seed_chain().await?;

    let dir = tempfile::tempdir()?;
    init_datadir(dir.path(), Some(false));

    // Distance-based receipt pruning routes receipt writes to MDBX on V1.
    // The distance is far larger than the seeded chain, so nothing actually
    // gets pruned and every receipt must survive the migration.
    std::fs::write(
        dir.path().join("reth.toml"),
        "[prune.segments]\nreceipts = { distance = 10064 }\n",
    )?;

    import_blocks(dir.path(), &seed.blocks_rlp)?;

    // Receipts landed in MDBX and not in static files, so this datadir
    // really exercises the receipt-copy phase of migrate-v2.
    parse_json(&db_stdout(dir.path(), &["get", "mdbx", "Receipts", "1"]));
    assert_eq!(
        db_stdout(dir.path(), &["get", "static-file", "receipts", "1"]),
        "",
        "a receipt-pruned V1 datadir must not have receipts in static files yet"
    );

    let out = migrate_v2(dir.path());
    assert!(out.status.success(), "migrate-v2 failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(storage_settings(dir.path()).contains("storage_v2: true"));

    // The EIP-1559 transfer receipt (tx 1) and the Berachain PoL system
    // receipt (tx 4) were copied into static files and still decode.
    for tx_num in ["1", "4"] {
        let receipt =
            parse_json(&db_stdout(dir.path(), &["get", "static-file", "receipts", tx_num]));
        assert!(receipt["logs"].is_array(), "receipt {tx_num} must decode, got: {receipt}");
    }
    Ok(())
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
