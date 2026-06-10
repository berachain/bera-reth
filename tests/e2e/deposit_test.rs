use crate::e2e::{berachain_payload_attributes_generator, test_signer};
use alloy_eips::Encodable2718;
use alloy_genesis::{Genesis, GenesisAccount};
use alloy_primitives::{Address, Bytes, TxKind, U256, address};
use alloy_rpc_types_eth::{TransactionInput, TransactionRequest};
use bera_reth::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth::tasks::Runtime;
use reth_chainspec::EthChainSpec;
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::{node::NodeTestContext, transaction::TransactionTestContext};
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::sync::Arc;

const DEPOSIT_CONTRACT: Address = address!("4242424242424242424242424242424242424242");
const DEPOSIT_REQUEST_TYPE: u8 = 0x00;
const DEPOSIT_SIZE: usize = 48 + 32 + 8 + 96 + 8;

// Amount encoded in the mock event: 32 ETH in Gwei = 0x773594000
const MOCK_AMOUNT_GWEI: u64 = 32_000_000_000;

// A time in the future for testing deposit requests before Osaka activation.
const OSAKA_TIME_FUTURE: u64 = 2_000_000_000;

// Runtime bytecode for a contract that emits a hardcoded Berachain Deposit event when called.
//
// Event: Deposit(bytes pubkey, bytes credentials, uint64 amount, bytes signature, uint64 index)
// Topic: keccak256("Deposit(bytes,bytes,uint64,bytes,uint64)")
//      = 0x68af751683498a9f9be59fe8b0d52a64dd155255d85cdb29fea30b1e3f891d46
//
// Emits with: pubkey=[0;48], credentials=[0;32], amount=32_000_000_000, signature=[0;96], index=0
//
// ABI-encoded log data (14 words = 448 bytes):
//   head[0] = 0xa0          (offset to pubkey data)
//   head[1] = 0x100         (offset to credentials data)
//   head[2] = 0x773594000   (amount in gwei)
//   head[3] = 0x140         (offset to signature data)
//   head[4] = 0x0           (index)
//   pubkey length = 0x30
//   pubkey data   = [0;64]  (48 bytes zero-padded to 64)
//   cred length   = 0x20
//   cred data     = [0;32]
//   sig length    = 0x60
//   sig data      = [0;96]
const DEPOSIT_EMITTER_BYTECODE: &str = concat!(
    // head[0] = 0xa0 at mem[0]
    "7f00000000000000000000000000000000000000000000000000000000000000a0",
    "6000",
    "52",
    // head[1] = 0x100 at mem[32]
    "7f0000000000000000000000000000000000000000000000000000000000000100",
    "6020",
    "52",
    // head[2] = 0x773594000 (32_000_000_000) at mem[64]
    "7f0000000000000000000000000000000000000000000000000000000773594000",
    "6040",
    "52",
    // head[3] = 0x140 at mem[96]
    "7f0000000000000000000000000000000000000000000000000000000000000140",
    "6060",
    "52",
    // head[4] = 0 (index) at mem[128] — zero, memory already zeroed, skip MSTORE
    // pubkey length = 48 at mem[160=0xa0]
    "6030",
    "60a0",
    "52",
    // pubkey data [0;64] at mem[192..256] — zero, skip
    // credentials length = 32 at mem[256=0x100]
    "6020",
    "610100",
    "52",
    // credentials data [0;32] at mem[288..320] — zero, skip
    // signature length = 96 at mem[320=0x140]
    "6060",
    "610140",
    "52",
    // signature data [0;96] at mem[352..448] — zero, skip
    // LOG1(offset=0, size=448=0x01c0, topic=0x68af...)
    "7f68af751683498a9f9be59fe8b0d52a64dd155255d85cdb29fea30b1e3f891d46",
    "6101c0", // PUSH2 448
    "5f",     // PUSH0 (offset = 0)
    "a1",     // LOG1
    // RETURN
    "5f",
    "5f",
    "f3",
);

async fn setup_deposit_test() -> eyre::Result<(Runtime, Arc<BerachainChainSpec>)> {
    setup_deposit_test_with_osaka(None).await
}

async fn setup_deposit_test_with_osaka(
    osaka_time: Option<u64>,
) -> eyre::Result<(Runtime, Arc<BerachainChainSpec>)> {
    let runtime = Runtime::with_existing_handle(tokio::runtime::Handle::current())?;
    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let mut genesis: Genesis = parse_genesis(&genesis_json)?;
    genesis.config.deposit_contract_address = Some(DEPOSIT_CONTRACT);
    if let Some(osaka_time) = osaka_time {
        genesis.config.osaka_time = Some(osaka_time);
    }
    let bytecode = Bytes::from(alloy_primitives::hex::decode(DEPOSIT_EMITTER_BYTECODE).unwrap());
    genesis
        .alloc
        .entry(DEPOSIT_CONTRACT)
        .and_modify(|a| a.code = Some(bytecode.clone()))
        .or_insert_with(|| GenesisAccount {
            code: Some(bytecode),
            nonce: Some(1),
            ..Default::default()
        });
    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));
    Ok((runtime, chain_spec))
}

#[tokio::test]
async fn test_eip6110_deposit_requests_active() -> eyre::Result<()> {
    let (runtime, chain_spec) = setup_deposit_test().await?;
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
    let chain_id = chain_spec.chain_id();

    let deposit_tx = TransactionRequest {
        to: Some(TxKind::Call(DEPOSIT_CONTRACT)),
        value: Some(U256::ZERO),
        input: TransactionInput::default(),
        gas: Some(100_000),
        chain_id: Some(chain_id),
        nonce: Some(0),
        max_fee_per_gas: Some(10_000_000_000),
        max_priority_fee_per_gas: Some(1_000_000_000),
        ..Default::default()
    };

    let tx_bytes: Bytes =
        TransactionTestContext::sign_tx(signer, deposit_tx).await.encoded_2718().into();
    ctx.rpc.inject_tx(tx_bytes).await?;

    let payload = ctx.advance_block().await?;

    let requests = payload.requests().expect("requests must be present when Prague is active");
    let deposit_request = requests
        .iter()
        .find(|r: &&Bytes| r.first() == Some(&DEPOSIT_REQUEST_TYPE))
        .expect("block must contain deposit requests after a deposit transaction");

    let deposit_data = &deposit_request[1..];
    assert_eq!(
        deposit_data.len(),
        DEPOSIT_SIZE,
        "deposit request must contain exactly one 192-byte deposit"
    );

    let pubkey_bytes = &deposit_data[..48];
    let amount_bytes: [u8; 8] = deposit_data[48 + 32..48 + 32 + 8].try_into().unwrap();
    let amount_gwei = u64::from_le_bytes(amount_bytes);

    assert_eq!(pubkey_bytes, &[0u8; 48], "pubkey must match emitted deposit");
    assert_eq!(amount_gwei, MOCK_AMOUNT_GWEI, "amount must be 32 ETH in Gwei");

    Ok(())
}

#[tokio::test]
async fn test_eip6110_no_deposits_without_deposit_tx() -> eyre::Result<()> {
    let (runtime, chain_spec) = setup_deposit_test().await?;
    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(runtime.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes_generator).await?;

    let payload = ctx.advance_block().await?;

    let requests = payload.requests().expect("requests must be present when Prague is active");
    let deposit_request =
        requests.iter().find(|r: &&Bytes| r.first() == Some(&DEPOSIT_REQUEST_TYPE));
    assert!(
        deposit_request.is_none(),
        "block without deposit transactions must not contain deposit requests"
    );

    Ok(())
}

#[tokio::test]
async fn test_eip6110_no_deposit_requests_before_osaka() -> eyre::Result<()> {
    let (runtime, chain_spec) = setup_deposit_test_with_osaka(Some(OSAKA_TIME_FUTURE)).await?;
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
    let chain_id = chain_spec.chain_id();

    let deposit_tx = TransactionRequest {
        to: Some(TxKind::Call(DEPOSIT_CONTRACT)),
        value: Some(U256::ZERO),
        input: TransactionInput::default(),
        gas: Some(100_000),
        chain_id: Some(chain_id),
        nonce: Some(0),
        max_fee_per_gas: Some(10_000_000_000),
        max_priority_fee_per_gas: Some(1_000_000_000),
        ..Default::default()
    };

    let tx_bytes: Bytes =
        TransactionTestContext::sign_tx(signer, deposit_tx).await.encoded_2718().into();
    let deposit_tx_hash = ctx.rpc.inject_tx(tx_bytes).await?;

    let payload = ctx.advance_block().await?;

    let block = payload.block();
    assert!(
        block.body().transactions.iter().any(|tx| *tx.hash() == deposit_tx_hash),
        "deposit transaction must be included so its deposit event is emitted before Osaka"
    );

    let requests = payload.requests().expect("requests must be present when Prague is active");
    assert!(
        requests.is_empty(),
        "deposit requests must be empty before Osaka even when a deposit event is emitted"
    );

    Ok(())
}
