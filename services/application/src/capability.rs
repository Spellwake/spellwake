//! Canonical random-key encoding shared by private account capabilities.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
/// Hash exactly32 decoded bytes; reject padding aliases before using an opaque key.
pub(crate) fn key_hash(raw: &str) -> Result<String, &'static str> {
    Ok(Sha256::digest(decode_key(raw)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
/// Shared canonical32-byte private capability decoder; never logs the supplied value.
pub(crate) fn decode_key(raw: &str) -> Result<[u8; 32], &'static str> {
    if raw.len() != 43 {
        return Err("invalid private capability");
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| "invalid private capability")?;
    if URL_SAFE_NO_PAD.encode(&bytes) != raw {
        return Err("invalid private capability");
    }
    bytes.try_into().map_err(|_| "invalid private capability")
}
