use crate::args::BerachainArgs;
use alloy_consensus::{EthereumTxEnvelope, SignableTransaction, TxEip1559};
use alloy_primitives::{Bytes, TxHash, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use eyre::Result;
use rand::Rng;
use reth_eth_wire_types::NetworkPrimitives;
use reth_network::NetworkHandle;
use reth_network_api::Peers;
use reth_network_peers::PeerId;
use rusqlite::{Connection, params};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use tracing::{debug, info, warn};

const CANARY_GAS_LIMIT: u64 = 21000;
const MAX_FEE_BUFFER_MULTIPLIER: u128 = 2;
const MIN_CANARY_VALUE: u64 = 1;
const MAX_CANARY_VALUE: u64 = 1000;
const LOOP_TICK_INTERVAL_SECS: u64 = 10;

pub trait NetworkOps: Peers {
    type Primitives: NetworkPrimitives;
    
    fn send_transactions(&self, peer_id: PeerId, msg: Vec<Arc<<Self::Primitives as NetworkPrimitives>::BroadcastedTransaction>>);
}

impl<N: NetworkPrimitives> NetworkOps for NetworkHandle<N> {
    type Primitives = N;
    
    fn send_transactions(&self, peer_id: PeerId, msg: Vec<Arc<<Self::Primitives as NetworkPrimitives>::BroadcastedTransaction>>) {
        NetworkHandle::send_transactions(self, peer_id, msg)
    }
}

struct ActiveCanary {
    tx_hash: TxHash,
    peer_id: PeerId,
    sent_at: Instant,
}

pub struct ProofOfGossipService<Network, P> {
    network: Network,
    provider: P,
    signer: PrivateKeySigner,
    chain_id: u64,
    db: Arc<Mutex<Connection>>,
    tested_peers: HashSet<PeerId>,
    active: Option<ActiveCanary>,
    nonce: u64,
    timeout: Duration,
}

impl<Network, P> ProofOfGossipService<Network, P>
where
    Network: NetworkOps<Primitives: NetworkPrimitives<BroadcastedTransaction = crate::transaction::BerachainTxEnvelope>> + Clone + Send + Sync + 'static,
    P: Provider + Clone + Send + Sync + 'static,
{
    pub async fn new_with_provider(
        network: Network,
        provider: P,
        chain_id: u64,
        datadir: PathBuf,
        args: &BerachainArgs,
    ) -> Result<Option<Self>> {
        let Some(private_key_hex) = &args.pog_private_key else {
            return Ok(None);
        };

        let signer = private_key_hex.parse::<PrivateKeySigner>()?;
        let address = signer.address();

        let nonce = provider.get_transaction_count(address).block_id(alloy_rpc_types::BlockId::latest()).await?;

        let db_path = datadir.join("proof_of_gossip.db");
        let db = Connection::open(&db_path)?;
        
        db.execute(
            "CREATE TABLE IF NOT EXISTS tested_peers (
                peer_id TEXT PRIMARY KEY,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            )",
            [],
        )?;

        db.execute("PRAGMA journal_mode=WAL", [])?;

        let tested_peers: HashSet<PeerId> = {
            let mut stmt = db.prepare("SELECT peer_id FROM tested_peers")?;
            stmt
                .query_map([], |row| {
                    let peer_id_str: String = row.get(0)?;
                    Ok(peer_id_str.parse::<PeerId>().map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                })?
                .collect::<Result<_, _>>()?
        };

        let db = Arc::new(Mutex::new(db));

        info!(
            target: "bera_reth::pog",
            address = %address,
            nonce = nonce,
            tested_peers = tested_peers.len(),
            "Proof of Gossip service initialized"
        );

        Ok(Some(Self {
            network,
            provider,
            signer,
            chain_id,
            db,
            tested_peers,
            active: None,
            nonce,
            timeout: Duration::from_secs(args.pog_timeout),
        }))
    }

    pub async fn run(mut self) {
        info!(target: "bera_reth::pog", "Starting Proof of Gossip service loop");

        loop {
            if let Err(e) = self.tick().await {
                warn!(target: "bera_reth::pog", error = %e, "Error in PoG service tick");
            }

            sleep(Duration::from_secs(LOOP_TICK_INTERVAL_SECS)).await;
        }
    }

    async fn tick(&mut self) -> Result<()> {
        if let Some(canary) = &self.active {
            if let Some(receipt) = self.provider.get_transaction_receipt(canary.tx_hash).await? {
                info!(
                    target: "bera_reth::pog",
                    peer_id = %canary.peer_id,
                    tx_hash = %canary.tx_hash,
                    block = receipt.block_number,
                    "Canary transaction confirmed"
                );

                self.persist_result(&canary.peer_id, canary.tx_hash, "confirmed")?;
                self.tested_peers.insert(canary.peer_id);
                self.active = None;
                self.nonce += 1;
            } else if canary.sent_at.elapsed() > self.timeout {
                warn!(
                    target: "bera_reth::pog",
                    peer_id = %canary.peer_id,
                    tx_hash = %canary.tx_hash,
                    elapsed_secs = canary.sent_at.elapsed().as_secs(),
                    "Canary transaction timed out"
                );

                self.persist_result(&canary.peer_id, canary.tx_hash, "timeout")?;
                self.tested_peers.insert(canary.peer_id);
                self.active = None;

                let address = self.signer.address();
                self.nonce = self.provider.get_transaction_count(address).block_id(alloy_rpc_types::BlockId::latest()).await?;
                
                debug!(
                    target: "bera_reth::pog",
                    nonce = self.nonce,
                    "Re-queried on-chain nonce after timeout"
                );
            }
        } else {
            let all_peers = self.network.get_all_peers().await?;
            
            if let Some(peer_info) = all_peers.iter().find(|p| !self.tested_peers.contains(&p.remote_id)) {
                let peer_id = peer_info.remote_id;
                
                let canary_tx = self.create_canary_tx().await?;
                let tx_hash = *canary_tx.hash();

                self.network.send_transactions(peer_id, vec![Arc::new(canary_tx)]);

                info!(
                    target: "bera_reth::pog",
                    peer_id = %peer_id,
                    tx_hash = %tx_hash,
                    nonce = self.nonce,
                    "Sent canary transaction to peer"
                );

                self.active = Some(ActiveCanary {
                    tx_hash,
                    peer_id,
                    sent_at: Instant::now(),
                });
            } else {
                debug!(
                    target: "bera_reth::pog",
                    connected_peers = all_peers.len(),
                    tested_peers = self.tested_peers.len(),
                    "No untested peers available"
                );
            }
        }

        Ok(())
    }

    async fn create_canary_tx(&self) -> Result<crate::transaction::BerachainTxEnvelope> {
        let to = self.signer.address();
        let value = rand::thread_rng().gen_range(MIN_CANARY_VALUE..=MAX_CANARY_VALUE);

        let latest_block = self.provider.get_block_by_number(
            alloy_rpc_types::BlockNumberOrTag::Latest,
        ).await?.ok_or_else(|| eyre::eyre!("Failed to fetch latest block"))?;

        let base_fee = latest_block.header.base_fee_per_gas.unwrap_or(1_000_000_000);
        let max_fee_per_gas = (base_fee as u128) * MAX_FEE_BUFFER_MULTIPLIER;

        let tx = TxEip1559 {
            chain_id: self.chain_id,
            nonce: self.nonce,
            gas_limit: CANARY_GAS_LIMIT,
            max_fee_per_gas,
            max_priority_fee_per_gas: 1_000_000_000,
            to: alloy_primitives::TxKind::Call(to),
            value: U256::from(value),
            access_list: Default::default(),
            input: Bytes::default(),
        };

        let signature = self.signer.sign_hash_sync(&tx.signature_hash())?;
        let signed = tx.into_signed(signature);
        let eth_envelope = EthereumTxEnvelope::Eip1559(signed);

        Ok(crate::transaction::BerachainTxEnvelope::Ethereum(eth_envelope))
    }

    fn persist_result(&self, peer_id: &PeerId, tx_hash: TxHash, result: &str) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs() as i64;

        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO tested_peers (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash.to_string(), result, timestamp],
        )?;

        Ok(())
    }
}

pub async fn new_pog_service<Network>(
    network: Network,
    provider_url: String,
    chain_id: u64,
    datadir: PathBuf,
    args: &BerachainArgs,
) -> Result<Option<ProofOfGossipService<Network, impl Provider + Clone + Send + Sync + 'static>>>
where
    Network: NetworkOps<Primitives: NetworkPrimitives<BroadcastedTransaction = crate::transaction::BerachainTxEnvelope>> + Clone + Send + Sync + 'static,
{
    let provider = ProviderBuilder::new().connect_http(provider_url.parse()?);
    ProofOfGossipService::new_with_provider(network, provider, chain_id, datadir, args).await
}

pub fn create_canary_tx(
    signer: &PrivateKeySigner,
    nonce: u64,
    chain_id: u64,
    base_fee: u128,
) -> Result<crate::transaction::BerachainTxEnvelope> {
    let to = signer.address();
    let value = rand::thread_rng().gen_range(MIN_CANARY_VALUE..=MAX_CANARY_VALUE);
    let max_fee_per_gas = base_fee * MAX_FEE_BUFFER_MULTIPLIER;

    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit: CANARY_GAS_LIMIT,
        max_fee_per_gas,
        max_priority_fee_per_gas: 1_000_000_000,
        to: alloy_primitives::TxKind::Call(to),
        value: U256::from(value),
        access_list: Default::default(),
        input: Bytes::default(),
    };

    let signature = signer.sign_hash_sync(&tx.signature_hash())?;
    let signed = tx.into_signed(signature);
    let eth_envelope = EthereumTxEnvelope::Eip1559(signed);

    Ok(crate::transaction::BerachainTxEnvelope::Ethereum(eth_envelope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::Transaction;
    use alloy_primitives::B256;
    use tempfile::NamedTempFile;

    #[test]
    fn test_canary_tx_construction() {
        let private_key = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let signer: PrivateKeySigner = private_key.parse().unwrap();
        let nonce = 42;
        let chain_id = 80094;
        let base_fee = 1_000_000_000;

        let tx = create_canary_tx(&signer, nonce, chain_id, base_fee).unwrap();

        let eth_envelope = match &tx {
            crate::transaction::BerachainTxEnvelope::Ethereum(eth) => eth,
            _ => panic!("Expected Ethereum transaction"),
        };

        let inner = match eth_envelope {
            EthereumTxEnvelope::Eip1559(signed) => signed,
            _ => panic!("Expected EIP-1559 transaction"),
        };

        assert_eq!(inner.to(), Some(signer.address()));
        assert!(inner.value() >= U256::from(MIN_CANARY_VALUE));
        assert!(inner.value() <= U256::from(MAX_CANARY_VALUE));
        assert_eq!(inner.gas_limit(), CANARY_GAS_LIMIT);
        assert_eq!(inner.nonce(), nonce);
        assert_eq!(inner.chain_id(), Some(chain_id));

        let recovered = inner.recover_signer().unwrap();
        assert_eq!(recovered, signer.address());
    }

    #[test]
    fn test_sqlite_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        let db = Connection::open(db_path).unwrap();
        db.execute(
            "CREATE TABLE IF NOT EXISTS tested_peers (
                peer_id TEXT PRIMARY KEY,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();

        let peer_id = PeerId::random();
        let tx_hash = B256::random();
        let timestamp = 1234567890i64;

        db.execute(
            "INSERT INTO tested_peers (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash.to_string(), "confirmed", timestamp],
        )
        .unwrap();

        let mut stmt = db.prepare("SELECT peer_id, tx_hash, result, tested_at FROM tested_peers").unwrap();
        let mut rows = stmt.query([]).unwrap();
        
        let row = rows.next().unwrap().unwrap();
        let loaded_peer_id: String = row.get(0).unwrap();
        let loaded_tx_hash: String = row.get(1).unwrap();
        let loaded_result: String = row.get(2).unwrap();
        let loaded_timestamp: i64 = row.get(3).unwrap();

        assert_eq!(loaded_peer_id, peer_id.to_string());
        assert_eq!(loaded_tx_hash, tx_hash.to_string());
        assert_eq!(loaded_result, "confirmed");
        assert_eq!(loaded_timestamp, timestamp);
    }

    #[test]
    fn test_sqlite_reload() {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        {
            let db = Connection::open(db_path).unwrap();
            db.execute(
                "CREATE TABLE IF NOT EXISTS tested_peers (
                    peer_id TEXT PRIMARY KEY,
                    tx_hash TEXT NOT NULL,
                    result TEXT NOT NULL,
                    tested_at INTEGER NOT NULL
                )",
                [],
            )
            .unwrap();

            let peer_id = PeerId::random();
            let tx_hash = B256::random();
            
            db.execute(
                "INSERT INTO tested_peers (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
                params![peer_id.to_string(), tx_hash.to_string(), "timeout", 9999999],
            )
            .unwrap();
        }

        let db = Connection::open(db_path).unwrap();
        let mut stmt = db.prepare("SELECT peer_id FROM tested_peers").unwrap();
        let count = stmt.query_map([], |_| Ok(())).unwrap().count();
        
        assert_eq!(count, 1);
    }

    #[test]
    fn test_sqlite_duplicate_peer_id() {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        let db = Connection::open(db_path).unwrap();
        db.execute(
            "CREATE TABLE IF NOT EXISTS tested_peers (
                peer_id TEXT PRIMARY KEY,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();

        let peer_id = PeerId::random();
        let tx_hash1 = B256::random();
        let tx_hash2 = B256::random();

        db.execute(
            "INSERT INTO tested_peers (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash1.to_string(), "confirmed", 1111111],
        )
        .unwrap();

        let result = db.execute(
            "INSERT INTO tested_peers (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash2.to_string(), "timeout", 2222222],
        );

        assert!(result.is_err());
    }
}
