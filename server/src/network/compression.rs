// massive_game_server/server/src/network/compression.rs

use anyhow::{anyhow, Result};

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
    let decoded =
        zstd::decode_all(payload).map_err(|err| anyhow!("zstd decode failed: {}", err))?;
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
}
