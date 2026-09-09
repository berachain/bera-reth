//! Targeted send, credit identity, signer serialization.

use super::{mock_network::MockPogNet, rpc_impl::PogApiImpl};
use crate::pog::SendMap;
use alloy_consensus::{EthereumTxEnvelope, SignableTransaction, TxEip1559};
use alloy_eips::Encodable2718;
use alloy_network::TxSigner;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use alloy_signer_local::PrivateKeySigner;
use reth_network_peers::PeerId;
use std::{net::SocketAddr, sync::Arc, time::Duration};

const KEY: &str = "0xfffdbb37105441e14b0ee6330d855d8504ff39e705c3afa8f859ac9865f99306";

struct UnusedProvider;

fn peer_hex(id: PeerId) -> String {
    alloy_primitives::hex::encode(id.as_slice())
}

fn harness(net: MockPogNet) -> (PogApiImpl<MockPogNet, UnusedProvider>, Arc<SendMap>) {
    let sends = Arc::new(SendMap::default());
    (PogApiImpl::new(net, UnusedProvider, Arc::clone(&sends)), sends)
}

async fn signed_raw(nonce: u64) -> (String, Address) {
    let bytes = alloy_primitives::hex::decode(KEY.trim_start_matches("0x")).unwrap();
    let signer = PrivateKeySigner::from_bytes(&B256::from_slice(&bytes)).unwrap();
    let from = signer.address();
    let mut tx = TxEip1559 {
        chain_id: 80094,
        nonce,
        gas_limit: 21_000,
        max_fee_per_gas: 1_000_000_000,
        max_priority_fee_per_gas: 1_000_000_000,
        to: TxKind::Call(from),
        value: U256::from(1),
        access_list: Default::default(),
        input: Bytes::new(),
    };
    let sig = signer.sign_transaction(&mut tx).await.unwrap();
    let env = EthereumTxEnvelope::<alloy_consensus::TxEip4844>::Eip1559(tx.into_signed(sig));
    (format!("0x{}", alloy_primitives::hex::encode(env.encoded_2718())), from)
}

fn err_msg<T>(r: jsonrpsee::core::RpcResult<T>) -> String {
    match r {
        Ok(_) => panic!("expected error"),
        Err(e) => e.message().to_string(),
    }
}

#[tokio::test]
async fn peers_use_session_enode_not_advertised() {
    let peer = PeerId::repeat_byte(0x11);
    let addr: SocketAddr = "51.68.187.101:30304".parse().unwrap();
    let net = MockPogNet::new().with_peer(peer, addr, false);
    let (api, _) = harness(net);
    let peers = api.peers_inner().await.unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].peer_id, peer_hex(peer));
    assert_eq!(peers[0].enode, format!("enode://{}@51.68.187.101:30304", peer_hex(peer)));
    assert_eq!(peers[0].direction, "outbound");
    assert_eq!(peers[0].client, "mock-peer");
    assert!(!peers[0].enode.contains("9.9.9.9"));
}

#[tokio::test]
async fn send_credits_peer_id_and_session_enode() {
    let peer = PeerId::repeat_byte(0x22);
    let addr: SocketAddr = "10.1.2.3:30303".parse().unwrap();
    let net = MockPogNet::new().with_peer(peer, addr, true);
    let (api, map) = harness(net.clone());
    let (raw, from) = signed_raw(0).await;
    let resp = api.send_raw_inner(peer_hex(peer), raw).await.unwrap();
    assert_eq!(resp.peer_id, peer_hex(peer));
    assert_eq!(resp.enode, format!("enode://{}@10.1.2.3:30303", peer_hex(peer)));
    let sent = net.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, peer);
    assert_eq!(sent[0].1, resp.tx_hash);
    let rows = api.sends_inner();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].from, from);
    assert_eq!(rows[0].nonce, 0);
    assert_eq!(rows[0].status, "sent");
    assert_eq!(rows[0].enode, resp.enode);
    assert_eq!(map.inflight_count(), 1);
}

#[tokio::test]
async fn second_send_same_signer_refused() {
    let peer = PeerId::repeat_byte(0x33);
    let addr: SocketAddr = "8.8.8.8:1".parse().unwrap();
    let net = MockPogNet::new().with_peer(peer, addr, false);
    let (api, _) = harness(net);
    let (raw0, _) = signed_raw(0).await;
    api.send_raw_inner(peer_hex(peer), raw0).await.unwrap();
    let (raw1, _) = signed_raw(1).await;
    let msg = err_msg(api.send_raw_inner(peer_hex(peer), raw1).await);
    assert!(msg.contains("inflight"), "{msg}");
}

#[tokio::test]
async fn send_refuses_syncing_and_disconnected() {
    let peer = PeerId::repeat_byte(0x44);
    let (api, _) = harness(MockPogNet::new().syncing(true));
    let (raw, _) = signed_raw(0).await;
    let msg = err_msg(api.send_raw_inner(peer_hex(peer), raw.clone()).await);
    assert!(msg.contains("syncing"), "{msg}");

    let (api, _) = harness(MockPogNet::new());
    let msg = err_msg(api.send_raw_inner(peer_hex(peer), raw).await);
    assert!(msg.contains("not connected"), "{msg}");
}

#[tokio::test]
async fn land_flips_status() {
    let peer = PeerId::repeat_byte(0x55);
    let addr: SocketAddr = "1.2.3.4:5".parse().unwrap();
    let net = MockPogNet::new().with_peer(peer, addr, false);
    let (api, map) = harness(net);
    let (raw, _) = signed_raw(0).await;
    let resp = api.send_raw_inner(peer_hex(peer), raw).await.unwrap();
    map.note_block(42, &[resp.tx_hash]);
    let row = &api.sends_inner()[0];
    assert_eq!(row.status, "landed");
    assert_eq!(row.block_number, Some(42));
}

#[tokio::test]
async fn timeout_then_late_land_via_rpc_map() {
    let peer = PeerId::repeat_byte(0x66);
    let addr: SocketAddr = "5.6.7.8:9".parse().unwrap();
    let net = MockPogNet::new().with_peer(peer, addr, false);
    let sends = Arc::new(SendMap::new(Duration::from_millis(1), Duration::from_secs(60)));
    let api = PogApiImpl::new(net, UnusedProvider, Arc::clone(&sends));
    let (raw, _) = signed_raw(0).await;
    let resp = api.send_raw_inner(peer_hex(peer), raw).await.unwrap();
    std::thread::sleep(Duration::from_millis(5));
    sends.expire_timeouts();
    assert_eq!(api.sends_inner()[0].status, "timeout");
    sends.note_block(99, &[resp.tx_hash]);
    assert_eq!(api.sends_inner()[0].status, "landed");
    assert_eq!(api.sends_inner()[0].block_number, Some(99));
}
