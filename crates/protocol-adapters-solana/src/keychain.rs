//! Solana signer bootstrap.
//!
//! Mirrors `crates/solver-main/src/messiah.rs` for the Solana side. The signing
//! key for the Mayan Swift `fulfill` path lives in the macOS keychain
//! (entry name `mamba-messiah-solana-key`). Falls back to the
//! `SOLANA_PRIVATE_KEY` env var with a WARN log when the keychain entry is
//! absent — same shape as the EVM bootstrap, except we accept both base58 and
//! hex secret forms (Solana CLI emits base58; some operators store the raw
//! 32-byte scalar as hex).
//!
//! No temp file is ever written. The raw key string is consumed inside
//! `load_solana_signer` and dropped before the function returns; only the
//! `SigningKey` escapes.

use anyhow::{anyhow, Context, Result};
use ed25519_dalek::SigningKey;
use tracing::warn;

use crate::send::SOLANA_PRIVATE_KEY_ENV;

const KEYCHAIN_ENTRY: &str = "mamba-messiah-solana-key";

/// Read the Solana signer secret from the macOS keychain. Returns the raw
/// key string. The caller must consume it into a `SigningKey` immediately
/// and drop the original.
fn read_keychain_secret() -> Result<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_ENTRY, "-w"])
        .output()
        .with_context(|| format!("invoke `security` for keychain entry {}", KEYCHAIN_ENTRY))?;
    if !out.status.success() {
        return Err(anyhow!(
            "keychain entry `{}` not found (stderr: {})",
            KEYCHAIN_ENTRY,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let key = String::from_utf8(out.stdout)
        .context("keychain output is not valid UTF-8")?
        .trim()
        .to_string();
    if key.is_empty() {
        return Err(anyhow!("keychain entry `{}` is empty", KEYCHAIN_ENTRY));
    }
    Ok(key)
}

/// Load the Solana `SigningKey` from the macOS keychain, falling back to the
/// `SOLANA_PRIVATE_KEY` env var (with a WARN log) if the keychain entry is
/// absent.
///
/// The raw key string is consumed and dropped inside this function; only the
/// derived `SigningKey` escapes.
pub fn load_solana_signer() -> Result<SigningKey> {
    let raw = match read_keychain_secret() {
        Ok(k) => k,
        Err(kc_err) => {
            warn!(
                "keychain entry `{}` unavailable ({}); falling back to {} env var",
                KEYCHAIN_ENTRY, kc_err, SOLANA_PRIVATE_KEY_ENV
            );
            std::env::var(SOLANA_PRIVATE_KEY_ENV).map_err(|_| {
                anyhow!(
                    "Solana signer not available: keychain entry `{}` absent and {} unset",
                    KEYCHAIN_ENTRY,
                    SOLANA_PRIVATE_KEY_ENV
                )
            })?
        }
    };
    let signer = parse_solana_secret(&raw)
        .map_err(|e| anyhow!("Solana signer secret is not a valid ed25519 key: {}", e))?;
    drop(raw);
    Ok(signer)
}

/// Parse a Solana signer secret. Accepts:
///   - Base58 64-byte keypair (the format `solana-keygen` produces)
///   - Base58 32-byte secret scalar
///   - Hex 32-byte secret scalar (with or without `0x` prefix)
pub(crate) fn parse_solana_secret(raw: &str) -> Result<SigningKey> {
    let trimmed = raw.trim();
    if let Ok(bytes) = bs58::decode(trimmed).into_vec() {
        if bytes.len() == 64 {
            let secret: [u8; 32] = bytes[..32]
                .try_into()
                .map_err(|_| anyhow!("secret scalar slice failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
        if bytes.len() == 32 {
            let secret: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow!("32-byte base58 secret failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    let clean = trimmed.trim_start_matches("0x");
    if let Ok(bytes) = hex::decode(clean) {
        if bytes.len() == 32 {
            let secret: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow!("hex 32-byte secret failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    Err(anyhow!(
        "Solana secret must be a base58-encoded 64-byte keypair, base58 32-byte secret, or hex 32-byte secret scalar; got {} chars",
        trimmed.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_base58_64_byte_keypair() {
        // Construct a deterministic 64-byte keypair: 32-byte secret + 32-byte pubkey.
        let secret = [7u8; 32];
        let signer = SigningKey::from_bytes(&secret);
        let pubkey = signer.verifying_key().to_bytes();
        let mut keypair = [0u8; 64];
        keypair[..32].copy_from_slice(&secret);
        keypair[32..].copy_from_slice(&pubkey);
        let b58 = bs58::encode(keypair).into_string();

        let parsed = parse_solana_secret(&b58).expect("parse b58 keypair");
        assert_eq!(parsed.to_bytes(), secret);
        assert_eq!(parsed.verifying_key().to_bytes(), pubkey);
    }

    #[test]
    fn parses_base58_32_byte_secret() {
        let secret = [9u8; 32];
        let b58 = bs58::encode(secret).into_string();
        let parsed = parse_solana_secret(&b58).expect("parse b58 32-byte secret");
        assert_eq!(parsed.to_bytes(), secret);
    }

    #[test]
    fn parses_hex_32_byte_secret() {
        let hex_key = "a".repeat(64); // 32 bytes of 0xaa
        let parsed = parse_solana_secret(&hex_key).expect("parse hex 32-byte secret");
        assert_eq!(parsed.to_bytes(), [0xaau8; 32]);
    }

    #[test]
    fn parses_hex_with_0x_prefix() {
        let hex_key = format!("0x{}", "b".repeat(64));
        let parsed = parse_solana_secret(&hex_key).expect("parse 0x-prefixed hex");
        assert_eq!(parsed.to_bytes(), [0xbbu8; 32]);
    }

    #[test]
    fn rejects_garbage() {
        let err = parse_solana_secret("not-a-key").unwrap_err();
        assert!(err.to_string().contains("Solana secret must be"), "{}", err);
    }

    #[test]
    fn rejects_wrong_length_hex() {
        // 16 bytes of hex — wrong length, must error
        let err = parse_solana_secret(&"f".repeat(32)).unwrap_err();
        assert!(err.to_string().contains("Solana secret must be"), "{}", err);
    }
}
