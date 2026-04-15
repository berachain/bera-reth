//! `beradmin` JSON-RPC namespace (WP1): detailed peers, node status, ban/penalize,
//! sentinel-driven PoG prepare/submit.

use crate::{
    chainspec::BerachainChainSpec,
    pog::{
        self, InflightProbe, PendingPrepare, PogAttributionStore, PogCoordinator, PogProvider,
        build_unsigned_canary, min_balance_for_canary, unsigned_tx_hex,
    },
    primitives::{BerachainBlock, BerachainHeader},
    transaction::{BerachainTxEnvelope, BerachainTxType},
};
use alloy_consensus::{EthereumTxEnvelope, Transaction};
use alloy_eips::{BlockHashOrNumber, Decodable2718};
use alloy_primitives::{Address, B256, U256, hex};
use async_trait::async_trait;
use jsonrpsee::{core::RpcResult, proc_macros::rpc, types::ErrorObjectOwned};
use reth::providers::{BlockReaderIdExt, ChainSpecProvider};
use reth_chainspec::{EthChainSpec, Head};
use reth_eth_wire_types::NetworkPrimitives;
use reth_network::NetworkHandle;
use reth_network_api::{FullNetwork, NetworkInfo, Peers, PeersInfo, ReputationChangeKind};
use reth_network_peers::PeerId;
use reth_network_types::peers::reputation::BANNED_REPUTATION;
use reth_primitives_traits::{SignerRecoverable, transaction::TxHashRef};
use reth_storage_api::{BlockReader, ReceiptProvider};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};

/// Targeted devp2p gossip for signed canary transactions (not broadcast RPC).
pub trait PogCanarySend: Send + Sync {
    fn pog_send_canary(&self, peer_id: PeerId, tx: std::sync::Arc<BerachainTxEnvelope>);
}

impl<N> PogCanarySend for NetworkHandle<N>
where
    N: NetworkPrimitives<BroadcastedTransaction = BerachainTxEnvelope> + Send + Sync + 'static,
{
    fn pog_send_canary(&self, peer_id: PeerId, tx: std::sync::Arc<BerachainTxEnvelope>) {
        NetworkHandle::send_transactions(self, peer_id, vec![tx]);
    }
}

/// One peer entry from `beradmin_detailedPeers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailedPeer {
    pub peer_id: String,
    pub enode: String,
    pub remote_addr: String,
    pub direction: String,
    pub client_version: String,
    pub chain_id: u64,
    pub genesis: B256,
    pub fork_id_hash: String,
    pub fork_id_next: u64,
    pub blockhash: B256,
    pub total_difficulty: Option<alloy_primitives::U256>,
    pub latest_block: Option<u64>,
    pub earliest_block: Option<u64>,
    pub reputation: i32,
    pub session_duration_mins: u64,
    pub backed_off: bool,
    pub severe_backoff_counter: u8,
    pub connection_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_fork_id_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_fork_id_next: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pog: Option<PogPeerStatus>,
}

pub use crate::pog::PogPeerStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatusResponse {
    pub chain_id: u64,
    pub genesis_hash: B256,
    pub fork_id_hash: String,
    pub fork_id_next: u64,
    pub head_number: u64,
    pub head_hash: B256,
    pub peer_count_inbound: usize,
    pub peer_count_outbound: usize,
    pub peer_count_total: usize,
    pub syncing: bool,
    pub client_version: String,
    pub network_id: u64,
    pub local_enode: String,
    pub local_peer_id: String,
    pub ban_threshold: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxAttribution {
    pub tx_hash: B256,
    pub from_peer_id: Option<String>,
    /// `(effective_gas_price − base_fee) × gas_used`
    pub effective_tip_wei: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedBlockAttributionResponse {
    pub block_number: u64,
    pub transactions: Vec<TxAttribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCanaryResponse {
    pub peer_id: String,
    pub enode: String,
    /// Hex-encoded EIP-1559 signing preimage (`0x02` + RLP) for sentinel to sign.
    pub unsigned_tx: String,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitCanaryResponse {
    pub tx_hash: B256,
    pub peer_id: String,
    pub enode: String,
}

#[rpc(server, namespace = "beradmin")]
pub trait BerAdminApi {
    #[method(name = "detailedPeers")]
    async fn detailed_peers(&self) -> RpcResult<Vec<DetailedPeer>>;

    #[method(name = "banPeer")]
    fn ban_peer(&self, peer_id: String) -> RpcResult<()>;

    #[method(name = "penalizePeer")]
    fn penalize_peer(&self, peer_id: String, value: i32) -> RpcResult<()>;

    #[method(name = "nodeStatus")]
    async fn node_status(&self) -> RpcResult<NodeStatusResponse>;

    /// Breaking change: `target_peer_id` is now required; the node no longer selects a target.
    #[method(name = "prepareCanary")]
    async fn prepare_canary(
        &self,
        signer_address: String,
        target_peer_id: String,
    ) -> RpcResult<PrepareCanaryResponse>;

    #[method(name = "submitCanary")]
    async fn submit_canary(&self, signed_tx: String) -> RpcResult<SubmitCanaryResponse>;

    #[method(name = "sealedBlockAttribution")]
    async fn sealed_block_attribution(
        &self,
        block_number: Option<u64>,
    ) -> RpcResult<SealedBlockAttributionResponse>;
}

fn parse_peer_id(s: &str) -> RpcResult<PeerId> {
    s.parse()
        .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid peer_id: {e}"), None::<()>))
}

fn fork_id_hash_hex(hash: [u8; 4]) -> String {
    format!("0x{}", hex::encode(hash))
}

fn peer_to_detailed(
    info: &reth_network_api::PeerInfo,
    peer: &reth_network_types::Peer,
    pog: Option<PogPeerStatus>,
) -> DetailedPeer {
    use reth_eth_wire_types::UnifiedStatus;
    let st: &UnifiedStatus = &info.status;
    let (disc_hash, disc_next) = match &peer.fork_id {
        Some(fid) => (Some(fork_id_hash_hex(fid.hash.0)), Some(fid.next)),
        None => (None, None),
    };
    DetailedPeer {
        peer_id: info.remote_id.to_string(),
        enode: info.enode.clone(),
        remote_addr: info.remote_addr.to_string(),
        direction: info.direction.to_string(),
        client_version: info.client_version.to_string(),
        chain_id: st.chain.id(),
        genesis: st.genesis,
        fork_id_hash: fork_id_hash_hex(st.forkid.hash.0),
        fork_id_next: st.forkid.next,
        blockhash: st.blockhash,
        total_difficulty: st.total_difficulty,
        latest_block: st.latest_block,
        earliest_block: st.earliest_block,
        reputation: peer.reputation,
        session_duration_mins: info.session_established.elapsed().as_secs() / 60,
        backed_off: peer.backed_off,
        severe_backoff_counter: peer.severe_backoff_counter,
        connection_state: format!("{:?}", peer.state),
        discovery_fork_id_hash: disc_hash,
        discovery_fork_id_next: disc_next,
        pog,
    }
}

pub struct BerAdminImpl<Network, Provider> {
    network: Network,
    provider: Provider,
    chain_spec: Arc<BerachainChainSpec>,
    client_version: String,
    pog: Arc<PogCoordinator>,
    attribution: Arc<PogAttributionStore>,
    pog_db: Option<Arc<pog::PogDb>>,
}

impl<Network, Provider> BerAdminImpl<Network, Provider> {
    pub fn new(
        network: Network,
        provider: Provider,
        chain_spec: Arc<BerachainChainSpec>,
        client_version: String,
        pog: Arc<PogCoordinator>,
        attribution: Arc<PogAttributionStore>,
    ) -> Self {
        let pog_db = pog::PogDb::open(pog.db_path()).ok().map(Arc::new);
        Self { network, provider, chain_spec, client_version, pog, attribution, pog_db }
    }
}

#[async_trait]
impl<Network, Provider> BerAdminApiServer for Arc<BerAdminImpl<Network, Provider>>
where
    Network: FullNetwork + PogCanarySend + Send + Sync + 'static,
    Provider: ChainSpecProvider<ChainSpec = BerachainChainSpec>
        + BlockReaderIdExt<Header = BerachainHeader>
        + BlockReader<Block = BerachainBlock>
        + ReceiptProvider<Receipt = reth_ethereum_primitives::Receipt<BerachainTxType>>
        + PogProvider
        + Send
        + Sync
        + 'static,
{
    async fn detailed_peers(&self) -> RpcResult<Vec<DetailedPeer>> {
        let peers = Peers::get_all_peers(&self.network)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?;

        let pog_statuses = self
            .pog_db
            .as_ref()
            .map(|db| db.all_peer_statuses())
            .unwrap_or_default();
        let handle = self.network.peers_handle();

        let mut result = Vec::with_capacity(peers.len());
        for info in &peers {
            let pog = pog_statuses.get(&info.remote_id.to_string()).cloned();
            let peer = handle.peer_by_id(info.remote_id).await.unwrap_or_else(|| {
                reth_network_types::Peer::new(reth_network_types::PeerAddr::from_tcp(
                    info.remote_addr,
                ))
            });
            result.push(peer_to_detailed(info, &peer, pog));
        }
        Ok(result)
    }

    fn ban_peer(&self, peer_id: String) -> RpcResult<()> {
        let id = parse_peer_id(&peer_id)?;
        Peers::reputation_change(&self.network, id, ReputationChangeKind::BadProtocol);
        Ok(())
    }

    fn penalize_peer(&self, peer_id: String, value: i32) -> RpcResult<()> {
        let id = parse_peer_id(&peer_id)?;
        Peers::reputation_change(&self.network, id, ReputationChangeKind::Other(value));
        Ok(())
    }

    async fn node_status(&self) -> RpcResult<NodeStatusResponse> {
        let chain_spec = self.chain_spec.as_ref();
        let chain_id = chain_spec.chain().id();
        let genesis_hash = chain_spec.genesis_hash();

        let latest = self
            .provider
            .latest_header()
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?
            .ok_or_else(|| ErrorObjectOwned::owned(-32000, "no best block", None::<()>))?;

        use alloy_consensus::Sealable;
        let header = latest.into_header();
        let head_number = header.number;
        let head_hash = Sealable::hash_slow(&header);

        let head = Head {
            number: head_number,
            hash: head_hash,
            timestamp: header.timestamp,
            difficulty: header.difficulty,
            total_difficulty: header.difficulty,
        };
        let fork_id = chain_spec.fork_id(&head);

        let peers = Peers::get_all_peers(&self.network)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?;
        let inbound = peers.iter().filter(|p| p.direction.is_incoming()).count();
        let outbound = peers.iter().filter(|p| p.direction.is_outgoing()).count();
        let total = peers.len();
        let syncing = NetworkInfo::is_syncing(&self.network);
        let node_record = PeersInfo::local_node_record(&self.network);
        let local_enode = node_record.to_string();
        let local_peer_id = node_record.id.to_string();

        Ok(NodeStatusResponse {
            chain_id,
            genesis_hash,
            fork_id_hash: fork_id_hash_hex(fork_id.hash.0),
            fork_id_next: fork_id.next,
            head_number,
            head_hash,
            peer_count_inbound: inbound,
            peer_count_outbound: outbound,
            peer_count_total: total,
            syncing,
            client_version: self.client_version.clone(),
            network_id: chain_id,
            local_enode,
            local_peer_id,
            ban_threshold: BANNED_REPUTATION,
        })
    }

    async fn prepare_canary(
        &self,
        signer_address: String,
        target_peer_id: String,
    ) -> RpcResult<PrepareCanaryResponse> {
        if NetworkInfo::is_syncing(&self.network) {
            return Err(ErrorObjectOwned::owned(
                -32000,
                "cannot prepare canary while node is syncing",
                None::<()>,
            ));
        }

        if self.pog.has_inflight() {
            return Err(ErrorObjectOwned::owned(
                -32000,
                "a canary probe is already in flight; wait for probe.result or timeout",
                None::<()>,
            ));
        }

        let signer: Address = signer_address
            .parse()
            .map_err(|_| ErrorObjectOwned::owned(-32602, "invalid signer_address", None::<()>))?;

        let target_id = parse_peer_id(&target_peer_id)?;

        if let Some(remaining) = self.pog.funding_backoff_active() {
            return Err(ErrorObjectOwned::owned(
                -32000,
                format!(
                    "signer wallet underfunded; backoff active (retry after {}s)",
                    remaining.as_secs().saturating_add(1)
                ),
                None::<()>,
            ));
        }

        let base_fee = match self.provider.latest_base_fee() {
            Ok(b) => b,
            Err(err) => {
                tracing::info!(target: "bera_reth::pog_probe", error = %err, "base fee fetch failed, using fallback");
                pog::CANARY_PRIORITY_FEE_WEI
            }
        };

        let balance = self
            .provider
            .account_balance(&signer)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?;
        let min_b = min_balance_for_canary(base_fee);
        match balance {
            Some(b) if b >= min_b => {
                self.pog.clear_funding_backoff();
            }
            other => {
                self.pog.record_underfunded();
                return Err(ErrorObjectOwned::owned(
                    -32000,
                    format!(
                        "insufficient funds for canary: balance={other:?} min_wei={min_b} signer={signer}"
                    ),
                    None::<()>,
                ));
            }
        }

        let peers = Peers::get_all_peers(&self.network)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?;

        let info = peers.iter().find(|p| p.remote_id == target_id).ok_or_else(|| {
            ErrorObjectOwned::owned(-32000, "target peer not connected", None::<()>)
        })?;

        let nonce = self
            .provider
            .account_nonce(&signer)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?
            .ok_or_else(|| {
                ErrorObjectOwned::owned(
                    -32000,
                    format!("signer account not found in state (fund {signer} first)"),
                    None::<()>,
                )
            })?;


        let (tx, value_wei) = build_unsigned_canary(signer, nonce, self.pog.chain_id, base_fee);
        let unsigned_tx = unsigned_tx_hex(&tx);

        self.pog.set_pending(PendingPrepare {
            peer_id: info.remote_id,
            enode: info.enode.clone(),
            nonce,
            signer,
            value_wei,
        });

        tracing::info!(
            target: "bera_reth::pog_probe",
            event = "probe.prepare",
            peer_id = %info.remote_id,
            enode = %info.enode,
            nonce,
            signer = %signer,
            value_wei,
            "prepareCanary ready"
        );

        Ok(PrepareCanaryResponse {
            peer_id: info.remote_id.to_string(),
            enode: info.enode.clone(),
            unsigned_tx,
            nonce,
        })
    }

    async fn submit_canary(&self, signed_tx: String) -> RpcResult<SubmitCanaryResponse> {
        if NetworkInfo::is_syncing(&self.network) {
            return Err(ErrorObjectOwned::owned(
                -32000,
                "cannot submit canary while node is syncing",
                None::<()>,
            ));
        }

        let raw = hex::decode(signed_tx.trim_start_matches("0x")).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("signed_tx is not valid hex: {e}"), None::<()>)
        })?;

        let mut slice = raw.as_slice();
        let envelope = BerachainTxEnvelope::decode_2718(&mut slice).map_err(|e| {
            tracing::info!(
                target: "bera_reth::pog_probe",
                event = "probe.result",
                outcome = "failed",
                reason = "decode",
                error = %e,
                "submitCanary decode failed"
            );
            ErrorObjectOwned::owned(-32602, format!("invalid signed transaction: {e}"), None::<()>)
        })?;

        let pending = self.pog.take_pending().ok_or_else(|| {
            ErrorObjectOwned::owned(
                -32000,
                "no pending prepareCanary; call prepareCanary first",
                None::<()>,
            )
        })?;

        let signer = match envelope.recover_signer() {
            Ok(s) => s,
            Err(e) => {
                self.pog.set_pending(pending);
                return Err(ErrorObjectOwned::owned(
                    -32602,
                    format!("cannot recover signer: {e}"),
                    None::<()>,
                ));
            }
        };

        if signer != pending.signer {
            self.pog.set_pending(pending);
            return Err(ErrorObjectOwned::owned(
                -32602,
                "signed tx signer does not match prepareCanary signer",
                None::<()>,
            ));
        }

        let (nonce, to, value) = match &envelope {
            BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip1559(s)) => {
                (s.nonce(), s.to(), s.value())
            }
            _ => {
                self.pog.set_pending(pending);
                return Err(ErrorObjectOwned::owned(
                    -32602,
                    "canary must be an EIP-1559 transaction",
                    None::<()>,
                ));
            }
        };

        if nonce != pending.nonce {
            let want = pending.nonce;
            self.pog.set_pending(pending);
            return Err(ErrorObjectOwned::owned(
                -32602,
                format!("nonce mismatch: tx has {nonce}, prepare had {want}"),
                None::<()>,
            ));
        }

        if to != Some(pending.signer) {
            self.pog.set_pending(pending);
            return Err(ErrorObjectOwned::owned(
                -32602,
                "canary must be self-transfer to signer",
                None::<()>,
            ));
        }

        if value != U256::from(pending.value_wei) {
            self.pog.set_pending(pending);
            return Err(ErrorObjectOwned::owned(
                -32602,
                "canary value does not match prepared transaction",
                None::<()>,
            ));
        }

        let peers = Peers::get_all_peers(&self.network)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?;
        if !peers.iter().any(|p| p.remote_id == pending.peer_id) {
            let pid = pending.peer_id;
            tracing::info!(
                target: "bera_reth::pog_probe",
                event = "probe.result",
                outcome = "failed",
                reason = "peer_disconnected",
                peer_id = %pid,
                "submitCanary target peer disconnected"
            );
            self.pog.set_pending(pending);
            return Err(ErrorObjectOwned::owned(
                -32000,
                format!("peer {pid} disconnected since prepareCanary"),
                None::<()>,
            ));
        }

        let tx_hash = *TxHashRef::tx_hash(&envelope);
        self.network.pog_send_canary(pending.peer_id, Arc::new(envelope));

        self.pog.set_inflight(InflightProbe {
            peer_id: pending.peer_id,
            enode: pending.enode.clone(),
            tx_hash,
            nonce: pending.nonce,
            value_wei: pending.value_wei,
            sent_at: Instant::now(),
        });

        tracing::info!(
            target: "bera_reth::pog_probe",
            event = "probe.dispatched",
            peer_id = %pending.peer_id,
            enode = %pending.enode,
            probe_id = %tx_hash,
            nonce = pending.nonce,
            value_wei = pending.value_wei,
            "canary delivered to peer via devp2p"
        );

        Ok(SubmitCanaryResponse {
            tx_hash,
            peer_id: pending.peer_id.to_string(),
            enode: pending.enode,
        })
    }

    async fn sealed_block_attribution(
        &self,
        block_number: Option<u64>,
    ) -> RpcResult<SealedBlockAttributionResponse> {
        let block_num = match block_number {
            Some(n) => n,
            None => self
                .attribution
                .sealed
                .lock()
                .map_err(|_| ErrorObjectOwned::owned(-32000, "lock poisoned", None::<()>))?
                .latest()
                .ok_or_else(|| {
                    ErrorObjectOwned::owned(
                        -32000,
                        "no sealed blocks tracked by this node",
                        None::<()>,
                    )
                })?,
        };

        let block = self
            .provider
            .block_by_number(block_num)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?
            .ok_or_else(|| {
                ErrorObjectOwned::owned(-32000, format!("block {block_num} not found"), None::<()>)
            })?;

        let in_sealed = self
            .attribution
            .sealed
            .lock()
            .map_err(|_| ErrorObjectOwned::owned(-32000, "lock poisoned", None::<()>))?
            .contains(block_num);

        if !in_sealed {
            return Ok(SealedBlockAttributionResponse {
                block_number: block_num,
                transactions: vec![],
            });
        }

        let base_fee = block.header.base_fee_per_gas.unwrap_or(0) as u128;

        let receipts = self
            .provider
            .receipts_by_block(BlockHashOrNumber::Number(block_num))
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))?
            .unwrap_or_default();

        let prov = self
            .attribution
            .provenance
            .lock()
            .map_err(|_| ErrorObjectOwned::owned(-32000, "lock poisoned", None::<()>))?;

        let mut prev_cumulative: u64 = 0;
        let mut transactions = Vec::with_capacity(block.body.transactions.len());
        for (tx, receipt) in block.body.transactions.iter().zip(receipts.iter()) {
            let tx_hash = *TxHashRef::tx_hash(tx);
            let gas_used = receipt.cumulative_gas_used.saturating_sub(prev_cumulative);
            prev_cumulative = receipt.cumulative_gas_used;

            let eff_price = tx.effective_gas_price(Some(base_fee as u64));
            let effective_tip_wei = eff_price.saturating_sub(base_fee) * gas_used as u128;

            let from_peer_id = prov.get(&tx_hash).map(|p| p.to_string());

            transactions.push(TxAttribution { tx_hash, from_peer_id, effective_tip_wei });
        }

        Ok(SealedBlockAttributionResponse { block_number: block_num, transactions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;

    fn make_test_peer(reputation: i32, backed_off: bool) -> DetailedPeer {
        DetailedPeer {
            peer_id: "0xabc".to_string(),
            enode: "enode://abc@1.2.3.4:30303".to_string(),
            remote_addr: "1.2.3.4:30303".to_string(),
            direction: "outgoing".to_string(),
            client_version: "bera-reth/1.0".to_string(),
            chain_id: 80094,
            genesis: B256::ZERO,
            fork_id_hash: "0xdeadbeef".to_string(),
            fork_id_next: 0,
            blockhash: B256::ZERO,
            total_difficulty: None,
            latest_block: Some(100),
            earliest_block: None,
            reputation,
            session_duration_mins: 5,
            backed_off,
            severe_backoff_counter: 0,
            connection_state: "Out".to_string(),
            discovery_fork_id_hash: None,
            discovery_fork_id_next: None,
            pog: None,
        }
    }

    #[test]
    fn serializes_reth_derived_fields() {
        let peer = make_test_peer(-50, false);
        let json = serde_json::to_value(&peer).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj["reputation"].as_i64(), Some(-50));
        assert_eq!(obj["sessionDurationMins"].as_u64(), Some(5));
        assert_eq!(obj["backedOff"].as_bool(), Some(false));
        assert_eq!(obj["severeBackoffCounter"].as_u64(), Some(0));
        assert_eq!(obj["connectionState"].as_str(), Some("Out"));
        assert!(!obj.contains_key("discoveryForkIdHash"));
    }

    #[test]
    fn roundtrip_with_discovery_fork_id() {
        let peer = DetailedPeer {
            discovery_fork_id_hash: Some("0x87654321".to_string()),
            discovery_fork_id_next: Some(1),
            reputation: 42,
            backed_off: true,
            severe_backoff_counter: 2,
            connection_state: "In".to_string(),
            total_difficulty: Some(U256::from(1000u64)),
            latest_block: Some(200),
            earliest_block: Some(0),
            pog: Some(PogPeerStatus {
                last_result: "seen".to_string(),
                failure_count: 0,
                last_tested_at: 12345,
            }),
            ..make_test_peer(42, true)
        };
        let json = serde_json::to_value(&peer).unwrap();
        let back: DetailedPeer = serde_json::from_value(json).unwrap();
        assert_eq!(back.reputation, 42);
        assert!(back.backed_off);
        assert_eq!(back.severe_backoff_counter, 2);
        assert_eq!(back.connection_state, "In");
        assert_eq!(back.discovery_fork_id_hash.as_deref(), Some("0x87654321"));
        assert_eq!(back.discovery_fork_id_next, Some(1));
        assert_eq!(back.pog.as_ref().map(|p| p.failure_count), Some(0));
    }

    // TP-5: nodeStatus includes ban_threshold
    #[test]
    fn node_status_ban_threshold_field_serializes() {
        let status = NodeStatusResponse {
            chain_id: 80094,
            genesis_hash: B256::ZERO,
            fork_id_hash: "0xdeadbeef".to_string(),
            fork_id_next: 0,
            head_number: 100,
            head_hash: B256::ZERO,
            peer_count_inbound: 0,
            peer_count_outbound: 0,
            peer_count_total: 0,
            syncing: false,
            client_version: "test".to_string(),
            network_id: 80094,
            local_enode: "enode://abc@1.2.3.4:30303".to_string(),
            local_peer_id: "0xabc".to_string(),
            ban_threshold: BANNED_REPUTATION,
        };
        let json = serde_json::to_value(&status).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("banThreshold"), "banThreshold must be present");
        assert_eq!(obj["banThreshold"].as_i64(), Some(BANNED_REPUTATION as i64));
        // BANNED_REPUTATION = 50 * -1024 = -51200
        assert_eq!(obj["banThreshold"].as_i64(), Some(-51200));
    }

    // TP-1: prepareCanary error cases (static logic tests)
    #[test]
    fn parse_peer_id_rejects_malformed() {
        assert!(parse_peer_id("notavalidpeerid").is_err());
        assert!(parse_peer_id("0xzzzz").is_err());
    }

    #[test]
    fn parse_peer_id_accepts_valid_hex() {
        let peer = PeerId::random();
        let s = peer.to_string();
        let parsed = parse_peer_id(&s).unwrap();
        assert_eq!(parsed, peer);
    }

    // TP-3/TP-4: sealedBlockAttribution logic (tip calculation unit test)
    #[test]
    fn effective_tip_calculation() {
        let base_fee: u128 = 1_000_000_000;
        let max_fee: u128 = 3_000_000_000;
        let max_priority: u128 = 500_000_000;
        let gas_used: u128 = 21000;

        let eff_price = (base_fee + max_priority).min(max_fee);
        let tip = eff_price.saturating_sub(base_fee) * gas_used;
        assert_eq!(eff_price, 1_500_000_000);
        assert_eq!(tip, 10_500_000_000_000u128);
    }

    #[test]
    fn sealed_block_attribution_response_serializes() {
        let r = SealedBlockAttributionResponse {
            block_number: 42,
            transactions: vec![TxAttribution {
                tx_hash: B256::ZERO,
                from_peer_id: Some("0xabc".to_string()),
                effective_tip_wei: 1234,
            }],
        };
        let json = serde_json::to_value(&r).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj["blockNumber"].as_u64(), Some(42));
        let txs = obj["transactions"].as_array().unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0]["fromPeerId"].as_str(), Some("0xabc"));
        assert_eq!(txs[0]["effectiveTipWei"].as_u64(), Some(1234));
    }

    #[test]
    fn sealed_block_attribution_null_from_peer_id() {
        let r = SealedBlockAttributionResponse {
            block_number: 1,
            transactions: vec![TxAttribution {
                tx_hash: B256::ZERO,
                from_peer_id: None,
                effective_tip_wei: 0,
            }],
        };
        let json = serde_json::to_value(&r).unwrap();
        let txs = json["transactions"].as_array().unwrap();
        assert!(txs[0]["fromPeerId"].is_null());
    }
}
