use alloy_primitives::bytes::Bytes;
use std::io::{self, Read};

pub trait FlashBlockDecoder<F>: Send + 'static {
    fn decode(&self, bytes: Bytes) -> eyre::Result<F>;
}

impl<F> FlashBlockDecoder<F> for ()
where
    F: serde::de::DeserializeOwned,
{
    fn decode(&self, bytes: Bytes) -> eyre::Result<F> {
        decode_flashblock(bytes)
    }
}

fn decode_flashblock<F>(bytes: Bytes) -> eyre::Result<F>
where
    F: serde::de::DeserializeOwned,
{
    let bytes = try_decompress(bytes)?;
    let payload: F =
        serde_json::from_slice(&bytes).map_err(|e| eyre::eyre!("failed to parse message: {e}"))?;
    Ok(payload)
}

// Well above worst-case block size to avoid clipping legitimate data,
// while still preventing decompression bombs from consuming gigabytes.
const MAX_DECOMPRESSED_SIZE: u64 = 32 * 1024 * 1024;

fn try_decompress(bytes: Bytes) -> eyre::Result<Bytes> {
    if bytes.trim_ascii_start().starts_with(b"{") {
        return Ok(bytes);
    }

    let decompressor = brotli::Decompressor::new(bytes.as_ref(), 4096);
    let mut decompressed = Vec::new();
    io::copy(&mut decompressor.take(MAX_DECOMPRESSED_SIZE), &mut decompressed)?;

    Ok(decompressed.into())
}
