// Berachain uses a custom deposit contract with a different event signature than
// the standard EIP-6110 deposit contract. This module replaces the default
// reth_evm::eth::eip6110 deposit parsing while preserving the same
// EIP-6110 convention: deposits are parsed from transaction logs and included
// directly as deposit requests in the execution payload's requests list
// (request type 0x00 per EIP-6110).
use alloy_consensus::TxReceipt;
use alloy_primitives::{Address, B256, Bytes, Log, b256};
use alloy_sol_types::{SolEvent, sol};
use reth_evm::block::BlockValidationError;

sol! {
    event Deposit(
        bytes pubkey,
        bytes credentials,
        uint64 amount,
        bytes signature,
        uint64 index
    );
}

/// keccak256("Deposit(bytes,bytes,uint64,bytes,uint64)")
///
/// Berachain uses a 5-field Deposit event that differs from Ethereum mainnet's
/// deposit contract. This constant must stay in sync with the `sol!` definition
/// above; see the `deposit_event_signature_matches_sol_type` test.
pub const DEPOSIT_EVENT_SIGNATURE: B256 =
    b256!("68af751683498a9f9be59fe8b0d52a64dd155255d85cdb29fea30b1e3f891d46");

const DEPOSIT_BYTES_SIZE: usize = 48 + 32 + 8 + 96 + 8;

fn accumulate_deposit_from_log(log: &Log<Deposit>, out: &mut Vec<u8>) {
    out.reserve(DEPOSIT_BYTES_SIZE);
    out.extend_from_slice(log.pubkey.as_ref());
    out.extend_from_slice(log.credentials.as_ref());
    out.extend_from_slice(&log.amount.to_le_bytes());
    out.extend_from_slice(log.signature.as_ref());
    out.extend_from_slice(&log.index.to_le_bytes());
}

pub fn parse_deposits_from_receipts<'a, I, R>(
    address: Address,
    receipts: I,
) -> Result<Bytes, BlockValidationError>
where
    I: IntoIterator<Item = &'a R>,
    R: TxReceipt<Log = Log> + 'a,
{
    let mut out = Vec::new();
    for receipt in receipts {
        for log in receipt.logs() {
            if log.address != address {
                continue;
            }
            if log.topics().first() != Some(&Deposit::SIGNATURE_HASH) {
                continue;
            }
            let decoded = Deposit::decode_log(log)
                .map_err(|err| BlockValidationError::DepositRequestDecode(err.to_string()))?;
            accumulate_deposit_from_log(&decoded, &mut out);
        }
    }
    Ok(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_event_signature_matches_sol_type() {
        assert_eq!(
            DEPOSIT_EVENT_SIGNATURE,
            Deposit::SIGNATURE_HASH,
            "DEPOSIT_EVENT_SIGNATURE must match the keccak256 of the sol! Deposit event"
        );
    }
}
