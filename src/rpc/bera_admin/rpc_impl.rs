//! `BerAdminImpl` and [`BerAdminApiServer`](super::BerAdminApiServer) implementation.

use super::BerAdminApiServer;
use super::{helpers::*, types::*};

use crate::{
    chainspec::BerachainChainSpec,
    pog::{
        self, InflightProbe, PendingPrepare, PogCoordinator, PogProvider, PogSqliteStore,
        build_unsigned_canary, min_balance_for_canary, unsigned_tx_hex,
    },
    primitives::{BerachainBlock, BerachainHeader},
    transaction::{BerachainTxEnvelope, BerachainTxType},
};
use alloy_consensus::{EthereumTxEnvelope, Transaction};
use alloy_eips::Decodable2718;
use alloy_primitives::{Address, U256, hex};
use async_trait::async_trait;
use jsonrpsee::{core::RpcResult, types::ErrorObjectOwned};
use reth::providers::{BlockReaderIdExt, ChainSpecProvider};
use reth_chainspec::{EthChainSpec, Head};
use reth_network_api::{FullNetwork, NetworkInfo, Peers, PeersInfo, ReputationChangeKind};
use reth_network_types::peers::reputation::BANNED_REPUTATION;
use reth_primitives_traits::{SignerRecoverable, transaction::TxHashRef};
use reth_storage_api::{BlockReader, ReceiptProvider};
use std::{sync::Arc, time::Instant};

pub struct BerAdminImpl<Network, Provider> {
    network: Network,
    provider: Provider,
    chain_spec: Arc<BerachainChainSpec>,
    client_version: String,
    pog: Arc<PogCoordinator>,
    store: Arc<PogSqliteStore>,
}

impl<Network, Provider> BerAdminImpl<Network, Provider> {
    pub fn new(
        network: Network,
        provider: Provider,
        chain_spec: Arc<BerachainChainSpec>,
        client_version: String,
        pog: Arc<PogCoordinator>,
    ) -> Self {
        let store = pog.store();
        Self { network, provider, chain_spec, client_version, pog, store }
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

        let pog_statuses = self.store.all_peer_statuses().map_err(|e| {
            ErrorObjectOwned::owned(
                -32000,
                format!("pog peer status query failed: {e}"),
                None::<()>,
            )
        })?;
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
        let local_peer_id = alloy_primitives::hex::encode(node_record.id.as_slice());

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

    /// Cursor-paginated sealed-tx-fact export (BERA-265 §5.7).
    ///
    /// Uses `PogSqliteStore::read_conn` exclusively (AC-R6). `effective_tip_wei` is
    /// returned as the ethereum-spec "Quantity" hex-`u128` encoding via
    /// `alloy_serde::quantity` on the `SealedTxFactRow` struct; storage holds the exact
    /// wire string so no re-encoding happens here.
    async fn export_sealed_tx_facts(
        &self,
        request: ExportSealedTxFactsRequest,
    ) -> RpcResult<ExportSealedTxFactsResponse> {
        let ExportSealedTxFactsRequest { after_id, limit } = request;

        // Range validation — reject, don't clamp. Server cap is CLI-configured
        // (`--sealed-fact-export-max-limit`, default 10_000; see cli_ext).
        if limit < 1 {
            return Err(ErrorObjectOwned::owned(-32602, "limit must be >= 1", None::<()>));
        }
        let server_max = pog::sealed_fact_config().export_max_limit;
        if limit > server_max {
            return Err(ErrorObjectOwned::owned(
                -32602,
                format!("limit {limit} exceeds server max {server_max}"),
                None::<()>,
            ));
        }

        let outcome = self.store.export_sealed_tx_facts(after_id, limit).map_err(|e| {
            ErrorObjectOwned::owned(-32000, format!("export query failed: {e}"), None::<()>)
        })?;

        let mut rows = Vec::with_capacity(outcome.rows.len());
        for r in outcome.rows {
            let tip = parse_u128_quantity(&r.effective_tip_wei).map_err(|e| {
                ErrorObjectOwned::owned(
                    -32000,
                    format!("stored tip for fact id={} is not a valid u128 quantity ({e})", r.id),
                    None::<()>,
                )
            })?;
            rows.push(SealedTxFactRow {
                id: r.id,
                sealed_block_number: r.sealed_block_number,
                tx_hash: r.tx_hash,
                first_peer_id: r.first_peer_id,
                first_heard_ms: r.first_heard_ms,
                effective_tip_wei: tip,
                tip_formula_version: r.tip_formula_version,
                extra_hears: Vec::new(),
            });
        }

        Ok(ExportSealedTxFactsResponse {
            rows,
            next_after_id: outcome.next_after_id,
            high_water_id: outcome.high_water_id,
            min_retained_id: outcome.min_retained_id,
            truncated: outcome.truncated,
        })
    }
}

fn parse_u128_quantity(s: &str) -> Result<u128, String> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    u128::from_str_radix(stripped, 16).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{B256, U256};
    use reth_network_peers::PeerId;

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
                last_tx_hash: "0xdeadbeef".to_string(),
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

    // nodeStatus includes ban_threshold
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

    // prepareCanary error cases (static logic tests)
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

    // ---- Wire-shape tests for BERA-265 types ----

    #[test]
    fn sealed_tx_fact_row_wire_shape_includes_empty_extra_hears() {
        let row = SealedTxFactRow {
            id: 12345,
            sealed_block_number: 98765,
            tx_hash: "0xdead".to_string(),
            first_peer_id: Some("0xabc".to_string()),
            first_heard_ms: 1_713_876_543_210,
            effective_tip_wei: 0x1bc16d674ec80000_u128,
            tip_formula_version: 1,
            extra_hears: Vec::new(),
        };
        let json = serde_json::to_value(&row).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj["id"].as_u64(), Some(12345));
        assert_eq!(obj["sealedBlockNumber"].as_u64(), Some(98765));
        assert_eq!(obj["firstPeerId"].as_str(), Some("0xabc"));
        assert_eq!(obj["effectiveTipWei"].as_str(), Some("0x1bc16d674ec80000"));
        assert_eq!(obj["tipFormulaVersion"].as_u64(), Some(1));
        let extras = obj["extraHears"].as_array().unwrap();
        assert!(extras.is_empty(), "AC-R4: extra_hears must be [] in v1");
    }

    #[test]
    fn sealed_tx_fact_row_null_peer_id_round_trip() {
        let row = SealedTxFactRow {
            id: 1,
            sealed_block_number: 1,
            tx_hash: "0x00".to_string(),
            first_peer_id: None,
            first_heard_ms: 0,
            effective_tip_wei: 0,
            tip_formula_version: 1,
            extra_hears: Vec::new(),
        };
        let json = serde_json::to_value(&row).unwrap();
        assert!(json["firstPeerId"].is_null());
        let back: SealedTxFactRow = serde_json::from_value(json).unwrap();
        assert_eq!(back.effective_tip_wei, 0);
        assert_eq!(back.first_peer_id, None);
    }

    #[test]
    fn export_response_wire_shape() {
        let resp = ExportSealedTxFactsResponse {
            rows: vec![],
            next_after_id: 0,
            high_water_id: 0,
            min_retained_id: 0,
            truncated: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        let obj = json.as_object().unwrap();
        for key in ["rows", "nextAfterId", "highWaterId", "minRetainedId", "truncated"] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
    }

    #[test]
    fn parse_u128_quantity_accepts_ethereum_spec_hex() {
        assert_eq!(parse_u128_quantity("0x0").unwrap(), 0);
        assert_eq!(parse_u128_quantity("0x1").unwrap(), 1);
        assert_eq!(parse_u128_quantity("0x1bc16d674ec80000").unwrap(), 0x1bc16d674ec80000_u128);
    }
}
