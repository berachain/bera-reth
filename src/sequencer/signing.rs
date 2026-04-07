//! BLS signing for flashblock preconfirmations.
//!
//! Implements the signing scheme for Berachain flashblocks:
//! `message = keccak256(domain || block_number || payload_id || index || diff_hash)`
//! where `domain = keccak256("BerachainPreconf-v1" || chain_id)`

use alloy_primitives::{B256, keccak256};
use blst::min_pk::{PublicKey, SecretKey, Signature};
use reth::rpc::types::engine::PayloadId;
use std::path::Path;

/// BLS signature bytes (96 bytes for BLS12-381 signatures).
pub type BlsSignature = [u8; 96];

/// BLS public key bytes (48 bytes for BLS12-381 public keys).
pub type BlsPublicKeyBytes = [u8; 48];

/// Domain separator version string.
const DOMAIN_VERSION: &[u8] = b"BerachainPreconf-v1";

/// BLS DST (Domain Separation Tag) matching beacon-kit's Proof of Possession scheme.
const BLS_DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

/// Errors that can occur during signing operations.
#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("invalid secret key")]
    InvalidSecretKey,
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("failed to read key file: {0}")]
    KeyFileError(#[from] std::io::Error),
    #[error("invalid key format: {0}")]
    InvalidKeyFormat(String),
}

/// BLS signer for flashblock preconfirmations.
#[derive(Clone)]
pub struct FlashblockSigner {
    secret_key: SecretKey,
    public_key: PublicKey,
    domain: B256,
}

impl std::fmt::Debug for FlashblockSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlashblockSigner")
            .field("public_key", &hex::encode(self.public_key.to_bytes()))
            .field("domain", &self.domain)
            .finish()
    }
}

impl FlashblockSigner {
    /// Create a new signer from a secret key and chain ID.
    pub fn new(secret_key: SecretKey, chain_id: u64) -> Self {
        let public_key = secret_key.sk_to_pk();
        let domain = compute_domain(chain_id);
        Self { secret_key, public_key, domain }
    }

    /// Create a signer from a hex-encoded secret key.
    pub fn from_hex(hex_key: &str, chain_id: u64) -> Result<Self, SigningError> {
        let key_bytes = hex::decode(hex_key.trim_start_matches("0x"))
            .map_err(|e| SigningError::InvalidKeyFormat(e.to_string()))?;

        if key_bytes.len() != 32 {
            return Err(SigningError::InvalidKeyFormat(format!(
                "expected 32 bytes, got {}",
                key_bytes.len()
            )));
        }

        let secret_key =
            SecretKey::from_bytes(&key_bytes).map_err(|_| SigningError::InvalidSecretKey)?;

        Ok(Self::new(secret_key, chain_id))
    }

    /// Load a signer from a key file (hex-encoded secret key).
    pub fn from_file(path: impl AsRef<Path>, chain_id: u64) -> Result<Self, SigningError> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_hex(contents.trim(), chain_id)
    }

    /// Get the public key bytes.
    pub fn public_key_bytes(&self) -> BlsPublicKeyBytes {
        self.public_key.to_bytes()
    }

    /// Sign a flashblock.
    pub fn sign_flashblock(
        &self,
        block_number: u64,
        payload_id: PayloadId,
        index: u64,
        diff_hash: B256,
    ) -> BlsSignature {
        let message =
            compute_signing_message(self.domain, block_number, payload_id, index, diff_hash);

        let signature = self.secret_key.sign(&message, BLS_DST, &[]);
        signature.to_bytes()
    }

    /// Verify a flashblock signature.
    pub fn verify(
        public_key: &BlsPublicKeyBytes,
        signature: &BlsSignature,
        chain_id: u64,
        block_number: u64,
        payload_id: PayloadId,
        index: u64,
        diff_hash: B256,
    ) -> Result<bool, SigningError> {
        let pk = PublicKey::from_bytes(public_key).map_err(|_| SigningError::InvalidPublicKey)?;
        let sig = Signature::from_bytes(signature).map_err(|_| SigningError::InvalidSignature)?;

        let domain = compute_domain(chain_id);
        let message = compute_signing_message(domain, block_number, payload_id, index, diff_hash);

        let result = sig.verify(true, &message, BLS_DST, &[], &pk, true);

        Ok(result == blst::BLST_ERROR::BLST_SUCCESS)
    }
}

/// Compute the domain separator for the given chain ID.
fn compute_domain(chain_id: u64) -> B256 {
    let mut data = Vec::with_capacity(DOMAIN_VERSION.len() + 8);
    data.extend_from_slice(DOMAIN_VERSION);
    data.extend_from_slice(&chain_id.to_be_bytes());
    keccak256(&data)
}

/// Compute the message to sign for a flashblock.
fn compute_signing_message(
    domain: B256,
    block_number: u64,
    payload_id: PayloadId,
    index: u64,
    diff_hash: B256,
) -> [u8; 32] {
    let mut data = Vec::with_capacity(32 + 8 + 8 + 8 + 32);
    data.extend_from_slice(domain.as_slice());
    data.extend_from_slice(&block_number.to_be_bytes());
    data.extend_from_slice(payload_id.0.as_slice());
    data.extend_from_slice(&index.to_be_bytes());
    data.extend_from_slice(diff_hash.as_slice());
    keccak256(&data).0
}

/// Compute the hash of flashblock transactions for signing.
pub fn compute_transactions_hash(transactions: &[impl AsRef<[u8]>]) -> B256 {
    let mut data = Vec::new();
    for tx in transactions {
        let tx_bytes = tx.as_ref();
        data.extend_from_slice(&(tx_bytes.len() as u64).to_be_bytes());
        data.extend_from_slice(tx_bytes);
    }
    keccak256(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAIN_ID: u64 = 80094;
    const BLOCK_NUMBER: u64 = 100;
    const INDEX: u64 = 0;

    fn test_signer() -> FlashblockSigner {
        let seed = [1u8; 32];
        FlashblockSigner::new(SecretKey::key_gen(&seed, &[]).unwrap(), CHAIN_ID)
    }

    #[test]
    fn test_sign_and_verify() {
        let signer = test_signer();
        let payload_id = PayloadId::new([1u8; 8]);
        let tx_hash = B256::repeat_byte(0x42);

        let signature = signer.sign_flashblock(BLOCK_NUMBER, payload_id, INDEX, tx_hash);

        let valid = FlashblockSigner::verify(
            &signer.public_key_bytes(),
            &signature,
            CHAIN_ID,
            BLOCK_NUMBER,
            payload_id,
            INDEX,
            tx_hash,
        )
        .unwrap();

        assert!(valid);
    }

    #[test]
    fn test_invalid_signature_fails_verification() {
        let signer = test_signer();
        let payload_id = PayloadId::new([1u8; 8]);
        let tx_hash = B256::repeat_byte(0x42);

        let signature = signer.sign_flashblock(BLOCK_NUMBER, payload_id, INDEX, tx_hash);

        let wrong_hash = B256::repeat_byte(0x43);
        let valid = FlashblockSigner::verify(
            &signer.public_key_bytes(),
            &signature,
            CHAIN_ID,
            BLOCK_NUMBER,
            payload_id,
            INDEX,
            wrong_hash,
        )
        .unwrap();

        assert!(!valid);
    }
}
