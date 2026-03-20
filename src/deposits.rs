use alloy_consensus::TxReceipt;
use alloy_primitives::{Address, Bytes, Log};
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
