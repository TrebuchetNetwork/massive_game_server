// massive_game_server/server/src/network/compression.rs

use anyhow::{anyhow, Result};
use std::io::Read;

#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub level: i32,
    pub max_uncompressed_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            level: 3,
            max_uncompressed_bytes: 8 * 1024 * 1024,
        }
    }
}

pub fn compress_payload(payload: &[u8], config: &CompressionConfig) -> Result<Vec<u8>> {
    zstd::encode_all(payload, config.level).map_err(|err| anyhow!("zstd encode failed: {}", err))
}

pub fn decompress_payload(payload: &[u8], config: &CompressionConfig) -> Result<Vec<u8>> {
    let decode_limit = config.max_uncompressed_bytes.saturating_add(1);
    let mut decoder =
        zstd::Decoder::new(payload).map_err(|err| anyhow!("zstd decode failed: {}", err))?;
    let mut limited_reader = decoder.by_ref().take(decode_limit as u64);
    let mut decoded = Vec::with_capacity(payload.len().min(decode_limit));
    limited_reader
        .read_to_end(&mut decoded)
        .map_err(|err| anyhow!("zstd decode failed: {}", err))?;
    if decoded.len() > config.max_uncompressed_bytes {
        return Err(anyhow!(
            "decompressed payload too large: {} > {}",
            decoded.len(),
            config.max_uncompressed_bytes
        ));
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let config = CompressionConfig::default();
        let payload = b"massive game server";
        let compressed = compress_payload(payload, &config).expect("compress");
        let restored = decompress_payload(&compressed, &config).expect("decompress");
        assert_eq!(restored, payload);
    }

    #[test]
    fn rejects_payload_over_limit_before_unbounded_allocation() {
        let config = CompressionConfig {
            level: 3,
            max_uncompressed_bytes: 64,
        };
        let payload = vec![42_u8; 2048];
        let compressed = compress_payload(&payload, &config).expect("compress");
        let err =
            decompress_payload(&compressed, &config).expect_err("must reject oversize payload");
        assert!(
            err.to_string().contains("decompressed payload too large"),
            "unexpected error: {err}"
        );
    }
}
