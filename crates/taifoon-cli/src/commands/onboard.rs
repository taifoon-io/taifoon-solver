//! `taifoon onboard` — bootstrap a solver identity end-to-end.
//!
//! Generates (or imports) an EVM keypair, prints the curl command to register
//! the wallet on Base Sepolia (real RPC call when a contract address is wired,
//! stub otherwise), writes `~/.taifoon/solver.toml`, and prints the env-var
//! snippets the operator needs to run `taifoon participate`.

use alloy::signers::local::PrivateKeySigner;
use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const BASE_SEPOLIA_CHAIN_ID: u64 = 84532;
const BASE_SEPOLIA_RPC: &str = "https://sepolia.base.org";

#[derive(Serialize, Deserialize, Debug)]
pub struct ChainWiring {
    pub chain_id: u64,
    pub rpc_url: String,
    pub registry_contract: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SolverConfig {
    pub solver_id: String,
    pub address: String,
    pub private_key: String,
    pub chain_wiring: ChainWiring,
    pub created_at: String,
}

pub struct OnboardArgs {
    pub import_key: Option<String>,
    pub registry_contract: Option<String>,
    pub solver_id: Option<String>,
    pub force: bool,
    pub json_mode: bool,
}

pub async fn run(args: OnboardArgs) -> Result<()> {
    let config_path = solver_config_path()?;

    if config_path.exists() && !args.force {
        return Err(anyhow!(
            "{} already exists. Re-run with --force to overwrite.",
            config_path.display()
        ));
    }

    let (signer, private_key_hex) = match args.import_key.as_deref() {
        Some(key) => {
            let cleaned = key.trim().trim_start_matches("0x");
            let signer: PrivateKeySigner = cleaned
                .parse()
                .map_err(|e| anyhow!("Invalid imported private key: {}", e))?;
            let hex_form = format!("0x{}", hex::encode(signer.to_bytes()));
            (signer, hex_form)
        }
        None => {
            let signer = PrivateKeySigner::random();
            let hex_form = format!("0x{}", hex::encode(signer.to_bytes()));
            (signer, hex_form)
        }
    };

    let address = format!("{:?}", signer.address());
    let solver_id = args
        .solver_id
        .clone()
        .unwrap_or_else(|| format!("solver-{}", &address[2..10]));

    let chain_wiring = ChainWiring {
        chain_id: BASE_SEPOLIA_CHAIN_ID,
        rpc_url: BASE_SEPOLIA_RPC.to_string(),
        registry_contract: args.registry_contract.clone(),
    };

    let config = SolverConfig {
        solver_id: solver_id.clone(),
        address: address.clone(),
        private_key: private_key_hex.clone(),
        chain_wiring,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let serialized = toml::to_string_pretty(&config)?;
    fs::write(&config_path, serialized)
        .with_context(|| format!("write {}", config_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&config_path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&config_path, perms)?;
    }

    let chain_wiring_json = serde_json::to_string(&config.chain_wiring)?;
    let register_cmd = build_register_command(&address, args.registry_contract.as_deref());

    if args.json_mode {
        let payload = serde_json::json!({
            "solver_id": solver_id,
            "address": address,
            "config_path": config_path.display().to_string(),
            "register_command": register_cmd,
            "env": {
                "SOLVER_PRIVATE_KEY": private_key_hex,
                "CHAIN_WIRING_JSON": chain_wiring_json,
            },
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("\n{}", "Taifoon Solver Onboarded".green().bold());
    println!("{}", "━".repeat(60));
    println!("  solver_id : {}", solver_id);
    println!("  address   : {}", address);
    println!("  chain     : Base Sepolia ({})", BASE_SEPOLIA_CHAIN_ID);
    println!("  config    : {}", config_path.display());
    println!("{}", "━".repeat(60));

    println!("\n{}", "Register on Base Sepolia:".cyan().bold());
    if args.registry_contract.is_none() {
        println!("  {}", "[stub — registry contract not yet deployed]".yellow());
    }
    println!("    {}", register_cmd);

    println!("\n{}", "Export these env vars:".cyan().bold());
    println!("    export SOLVER_PRIVATE_KEY='{}'", private_key_hex);
    println!("    export CHAIN_WIRING_JSON='{}'", chain_wiring_json);

    println!(
        "\n{}",
        "WARNING: solver.toml contains your private key — keep it secret."
            .yellow()
            .bold()
    );

    Ok(())
}

fn solver_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not resolve $HOME"))?;
    Ok(home.join(".taifoon").join("solver.toml"))
}

fn build_register_command(address: &str, registry_contract: Option<&str>) -> String {
    match registry_contract {
        Some(contract) => format!(
            "curl -X POST {rpc} -H 'content-type: application/json' \
             --data '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"eth_call\",\
             \"params\":[{{\"to\":\"{contract}\",\
             \"data\":\"0x{selector}{padded_addr}\"}},\"latest\"]}}'",
            rpc = BASE_SEPOLIA_RPC,
            contract = contract,
            selector = "1aa3a008", // register(address) — placeholder until ABI is published
            padded_addr = pad_address(address),
        ),
        None => format!(
            "# stub — registry not deployed yet. Once it is:\n\
             #   cast send <REGISTRY_CONTRACT> 'register(address)' {addr} \\\n\
             #     --rpc-url {rpc} --private-key $SOLVER_PRIVATE_KEY",
            addr = address,
            rpc = BASE_SEPOLIA_RPC,
        ),
    }
}

fn pad_address(address: &str) -> String {
    let cleaned = address.trim_start_matches("0x");
    format!("{:0>64}", cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_address_left_pads_to_32_bytes() {
        let padded = pad_address("0xabcdef");
        assert_eq!(padded.len(), 64);
        assert!(padded.ends_with("abcdef"));
        assert!(padded.starts_with("0000"));
    }

    #[test]
    fn build_register_command_stub_when_no_contract() {
        let cmd = build_register_command("0xdead", None);
        assert!(cmd.contains("stub"));
        assert!(cmd.contains("0xdead"));
    }

    #[test]
    fn build_register_command_includes_contract_and_address() {
        let cmd = build_register_command("0x1234", Some("0xCAFE"));
        assert!(cmd.contains("0xCAFE"));
        assert!(cmd.contains("eth_call"));
    }
}
