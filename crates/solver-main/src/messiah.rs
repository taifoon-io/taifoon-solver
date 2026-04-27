//! MESSIAH wallet bootstrap.
//!
//! The signing key for the estimate / fill pipeline lives ONLY in the macOS
//! keychain (entry name `mamba-messiah-key`). It is never written to disk,
//! never passed through environment variables, never logged, and never
//! returned in workflow outputs. After loading we immediately consume the
//! string into an `alloy` signer and let the original `String` drop —
//! after that point the only artifact that escapes is the derived public
//! address.

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use anyhow::{anyhow, Context, Result};
use std::str::FromStr;

const KEYCHAIN_ENTRY: &str = "mamba-messiah-key";

/// Read the MESSIAH private key from the macOS keychain. Returns the raw key
/// string. The caller must dispose of it as quickly as possible (immediately
/// pass it to `PrivateKeySigner::from_str` and drop the original).
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

/// Load the MESSIAH signer from the macOS keychain. The raw key is consumed
/// inside this function and the only thing that escapes is the signer.
pub fn load_messiah_signer() -> Result<PrivateKeySigner> {
    let key = read_keychain_secret()?;
    let signer = PrivateKeySigner::from_str(&key)
        .map_err(|e| anyhow!("MESSIAH key is not a valid secp256k1 private key: {}", e))?;
    // `key` drops here.
    drop(key);
    Ok(signer)
}

/// Convenience: load the MESSIAH signer and return the derived public address
/// only. This is the form used for estimate-only flows where we never need the
/// signing capability — but we still want to confirm the keychain entry parses.
pub fn load_messiah_address() -> Result<Address> {
    let signer = load_messiah_signer()?;
    Ok(signer.address())
}
