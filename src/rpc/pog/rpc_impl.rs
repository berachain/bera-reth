//! `PogApiServer` implementation.

use super::{PogApiServer, network::PogNet, types::*};
use crate::{
    pog::{
        self, SendMap, SendRecord, SendStatus, direction_label, record_penalize,
        record_send_result, refresh_inflight_gauge, refresh_peer_subnet_gauges, session_enode,
        subnet_24,
    },
    primitives::BerachainHeader,
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::{EthereumTxEnvelope, Transaction, transaction::SignerRecoverable};
use alloy_eips::Decodable2718;
use alloy_primitives::hex;
use async_trait::async_trait;
use jsonrpsee::{core::RpcResult, types::ErrorObjectOwned};
use reth::providers::BlockReaderIdExt;
use reth_network_peers::PeerId;
use reth_primitives_traits::transaction::TxHashRef;
use std::sync::Arc;

pub struct PogApiImpl<Network, Provider> {
    network: Network,
    provider: Provider,
    sends: Arc<SendMap>,
}

impl<Network, Provider> PogApiImpl<Network, Provider> {
    pub fn new(network: Network, provider: Provider, sends: Arc<SendMap>) -> Self {
        Self { network, provider, sends }
    }
}

fn rpc_err(msg: impl Into<String>) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(-32000, msg.into(), None::<()>)
}

fn parse_peer_id(s: &str) -> RpcResult<PeerId> {
    s.trim()
        .trim_start_matches("0x")
        .parse()
        .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid peer_id: {e}"), None::<()>))
}

fn decode_raw_tx(raw_tx: &str) -> RpcResult<BerachainTxEnvelope> {
    let hex_str = raw_tx.trim().trim_start_matches("0x");
    let bytes = hex::decode(hex_str)
        .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid tx hex: {e}"), None::<()>))?;
    let mut buf: &[u8] = &bytes;
    let eth =
        EthereumTxEnvelope::<alloy_consensus::TxEip4844>::decode_2718(&mut buf).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("invalid transaction: {e}"), None::<()>)
        })?;
    Ok(BerachainTxEnvelope::Ethereum(eth))
}

fn peer_id_hex(id: PeerId) -> String {
    alloy_primitives::hex::encode(id.as_slice())
}

impl<Network, Provider> PogApiImpl<Network, Provider>
where
    Network: PogNet,
{
    pub async fn peers_inner(&self) -> RpcResult<Vec<PogPeer>> {
        let peers = self.network.get_all_peers().await.map_err(|e| rpc_err(e.to_string()))?;
        let occupancy: Vec<(String, String)> = peers
            .iter()
            .map(|p| {
                (subnet_24(p.remote_addr), direction_label(p.direction.is_incoming()).to_string())
            })
            .collect();
        refresh_peer_subnet_gauges(&occupancy);

        Ok(peers
            .iter()
            .map(|p| PogPeer {
                peer_id: peer_id_hex(p.remote_id),
                enode: session_enode(p.remote_id, p.remote_addr),
                direction: direction_label(p.direction.is_incoming()).to_string(),
                client: p.client_version.to_string(),
            })
            .collect())
    }

    pub async fn send_raw_inner(
        &self,
        peer_id: String,
        raw_tx: String,
    ) -> RpcResult<SendRawTransactionResponse> {
        if self.network.is_syncing() {
            record_send_result("unknown", "refused");
            return Err(rpc_err("cannot send while node is syncing"));
        }

        let target = parse_peer_id(&peer_id)?;
        let envelope = decode_raw_tx(&raw_tx)?;
        let from = envelope
            .recover_signer()
            .map_err(|e| rpc_err(format!("cannot recover signer: {e}")))?;
        let nonce = envelope.nonce();
        let tx_hash = *TxHashRef::tx_hash(&envelope);

        if self.sends.signer_inflight(from) {
            record_send_result("unknown", "refused");
            return Err(rpc_err("signer already has an inflight send"));
        }

        let peers = self.network.get_all_peers().await.map_err(|e| rpc_err(e.to_string()))?;
        let info = peers.iter().find(|p| p.remote_id == target).ok_or_else(|| {
            record_send_result("unknown", "refused");
            rpc_err("target peer not connected")
        })?;

        let enode = session_enode(info.remote_id, info.remote_addr);
        let subnet = subnet_24(info.remote_addr);
        self.network.send_raw(info.remote_id, Arc::new(envelope));
        self.sends.insert(SendRecord {
            tx_hash,
            peer_id: info.remote_id,
            enode: enode.clone(),
            subnet: subnet.clone(),
            from,
            nonce,
            sent_at: std::time::Instant::now(),
            sent_at_unix: pog::now_unix(),
            status: SendStatus::Sent,
            block_number: None,
        });
        record_send_result(&subnet, "sent");
        refresh_inflight_gauge(self.sends.inflight_count());

        Ok(SendRawTransactionResponse { tx_hash, peer_id: peer_id_hex(info.remote_id), enode })
    }

    pub async fn penalize_inner(&self, peer_id: String) -> RpcResult<PenalizeResponse> {
        let target = parse_peer_id(&peer_id)?;
        let peers = self.network.get_all_peers().await.map_err(|e| rpc_err(e.to_string()))?;
        let session = peers.iter().find(|p| p.remote_id == target);
        let subnet = session.map(|p| subnet_24(p.remote_addr)).unwrap_or_else(|| "unknown".into());

        self.network.penalize(target);
        record_penalize(&subnet);

        Ok(PenalizeResponse { peer_id: peer_id_hex(target), connected: session.is_some() })
    }

    pub fn sends_inner(&self) -> Vec<PogSend> {
        self.sends
            .snapshot()
            .into_iter()
            .map(|r| PogSend {
                tx_hash: r.tx_hash,
                peer_id: peer_id_hex(r.peer_id),
                enode: r.enode,
                from: r.from,
                nonce: r.nonce,
                sent_at: r.sent_at_unix,
                status: r.status.as_str().to_string(),
                block_number: r.block_number,
            })
            .collect()
    }
}

#[async_trait]
impl<Network, Provider> PogApiServer for PogApiImpl<Network, Provider>
where
    Network: PogNet,
    Provider: BlockReaderIdExt<Header = BerachainHeader> + Send + Sync + 'static,
{
    async fn peers(&self) -> RpcResult<Vec<PogPeer>> {
        self.peers_inner().await
    }

    async fn send_raw_transaction(
        &self,
        peer_id: String,
        raw_tx: String,
    ) -> RpcResult<SendRawTransactionResponse> {
        self.send_raw_inner(peer_id, raw_tx).await
    }

    async fn penalize(&self, peer_id: String) -> RpcResult<PenalizeResponse> {
        self.penalize_inner(peer_id).await
    }

    fn sends(&self) -> RpcResult<Vec<PogSend>> {
        Ok(self.sends_inner())
    }

    async fn node_status(&self) -> RpcResult<PogNodeStatus> {
        let latest = self
            .provider
            .latest_header()
            .map_err(|e| rpc_err(e.to_string()))?
            .ok_or_else(|| rpc_err("no best block"))?;
        use alloy_consensus::{BlockHeader, Sealable};
        let header = latest.into_header();
        Ok(PogNodeStatus {
            syncing: self.network.is_syncing(),
            head_number: header.number(),
            head_hash: Sealable::hash_slow(&header),
            sends_inflight: self.sends.inflight_count(),
        })
    }
}
