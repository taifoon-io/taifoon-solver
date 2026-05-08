//! Wallet management for Taifoon solver

use alloy::signers::local::PrivateKeySigner;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct Wallet;

#[derive(Serialize, Deserialize)]
pub struct WalletStatus {
    pub address: String,
    pub chain_id: u64,
    pub balance_eth: String,
    pub balance_wei: String,
    pub authorized: bool,
}

#[derive(Serialize)]
pub struct WalletGenerated {
    pub address: String,
    pub private_key: String,
    pub warning: String,
}

impl Wallet {
    pub fn from_private_key(key: &str) -> Result<PrivateKeySigner> {
        let key_clean = key.trim().trim_start_matches("0x");
        let signer: PrivateKeySigner = key_clean.parse()
            .map_err(|e| anyhow!("Invalid private key: {}", e))?;
        Ok(signer)
    }

    pub fn generate_new() -> Result<(PrivateKeySigner, String)> {
        let signer = PrivateKeySigner::random();
        let private_key_hex = format!("0x{}", hex::encode(signer.to_bytes()));
        Ok((signer, private_key_hex))
    }
}

pub async fn status(
    private_key: &str,
    chain: Option<u64>,
    _spinner_url: &str,
    json_mode: bool,
) -> Result<()> {
    let signer = Wallet::from_private_key(private_key)?;
    let address = signer.address();
    let chain_id = chain.unwrap_or(42161); // Default to Arbitrum

    // For now, we'll return a placeholder status
    // In production, this would call RPC to get actual balance
    let status = WalletStatus {
        address: format!("{:?}", address),
        chain_id,
        balance_eth: "0.0".to_string(),
        balance_wei: "0".to_string(),
        authorized: true,
    };

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("Wallet Status:");
        println!("  Address: {}", status.address);
        println!("  Chain: {}", status.chain_id);
        println!("  Balance: {} ETH ({} wei)", status.balance_eth, status.balance_wei);
        println!("  Authorized: {}", if status.authorized { "✓" } else { "✗" });
    }

    Ok(())
}

pub async fn generate(json_mode: bool) -> Result<()> {
    let (signer, private_key_hex) = Wallet::generate_new()?;
    let address = signer.address();

    let generated = WalletGenerated {
        address: format!("{:?}", address),
        private_key: private_key_hex.clone(),
        warning: "KEEP THIS PRIVATE KEY SECRET! Never share it or commit it to version control.".to_string(),
    };

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&generated)?);
    } else {
        println!("\n🔐 Generated New Wallet");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Address:     {}", generated.address);
        println!("Private Key: {}", generated.private_key);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("\n⚠️  WARNING: KEEP THIS PRIVATE KEY SECRET!");
        println!("   Never share it or commit it to version control.\n");
    }

    Ok(())
}

pub async fn address(private_key: &str, json_mode: bool) -> Result<()> {
    let signer = Wallet::from_private_key(private_key)?;
    let address = signer.address();

    if json_mode {
        println!("{}", json!({ "address": format!("{:?}", address) }));
    } else {
        println!("Address: {:?}", address);
    }

    Ok(())
}
