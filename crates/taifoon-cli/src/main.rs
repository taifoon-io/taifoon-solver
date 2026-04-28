//! Taifoon CLI - Command-line interface for cross-chain solver operations
//!
//! ## Crown Jewel Command
//! ```bash
//! taifoon participate --private-key 0x... --auto
//! ```
//!
//! ## Agent-Friendly Design
//! - All commands output JSON when --json flag is used
//! - Exit codes: 0 = success, 1 = error, 2 = no opportunities
//! - Autonomous operation mode for AI agents

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

mod wallet;
mod monitor;
mod execute;
mod test_mode;
mod commands;

use wallet::Wallet;

// ── CLI Structure ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "taifoon")]
#[command(about = "Taifoon Cross-Chain Solver CLI", long_about = None)]
#[command(version)]
struct Cli {
    /// Output JSON instead of human-readable format
    #[arg(long, global = true)]
    json: bool,

    /// Razor / WARMBED gas API URL (env: WARMBED_API_URL or SPINNER_API_URL)
    #[arg(long, env = "WARMBED_API_URL", default_value = "https://api.taifoon.dev")]
    spinner_url: String,

    /// Genome SSE stream URL
    #[arg(long, env = "GENOME_SSE_URL", default_value = "https://api.taifoon.dev/api/genome/subscribe/sse")]
    genome_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 👑 CROWN JEWEL: Authorize with private key and start actively participating
    ///
    /// This is the primary command for autonomous solver operation.
    /// Use --auto to enable full autonomous mode where the solver will:
    /// 1. Monitor genome stream for profitable intents
    /// 2. Estimate gas costs via Spinner API
    /// 3. Execute fills when profitable
    /// 4. Claim settlements automatically
    ///
    /// Example (simulation):
    /// $ taifoon participate --private-key 0x... --dry-run
    ///
    /// Example (live, autonomous):
    /// $ taifoon participate --private-key 0x... --auto --min-profit 0.50
    Participate {
        /// Private key (hex, with or without 0x prefix)
        #[arg(long, env = "SOLVER_PRIVATE_KEY")]
        private_key: String,

        /// Enable full autonomous mode (no confirmations)
        #[arg(long)]
        auto: bool,

        /// Minimum profit in USD to execute
        #[arg(long, default_value = "0.10")]
        min_profit: f64,

        /// Protocol filter (across, debridge, lifi, mayan, or "all")
        #[arg(long, default_value = "all")]
        protocol: String,

        /// Dry-run mode (simulate, don't broadcast)
        #[arg(long)]
        dry_run: bool,

        /// Maximum concurrent fills
        #[arg(long, default_value = "3")]
        max_concurrent: usize,
    },

    /// Check wallet balance and authorization status
    ///
    /// Example:
    /// $ taifoon wallet status --private-key 0x...
    /// $ taifoon wallet status --private-key 0x... --json
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },

    /// Monitor genome stream for intents (read-only)
    ///
    /// Example:
    /// $ taifoon monitor --protocol across
    /// $ taifoon monitor --json | jq '.intents[] | select(.profitable)'
    Monitor {
        /// Protocol filter
        #[arg(long)]
        protocol: Option<String>,

        /// Stop after N intents
        #[arg(long)]
        limit: Option<usize>,

        /// Show only profitable opportunities
        #[arg(long)]
        profitable_only: bool,
    },

    /// Execute a single fill transaction
    ///
    /// Example:
    /// $ taifoon execute --intent-id across:0x123... --private-key 0x... --dry-run
    Execute {
        /// Intent ID from genome stream
        #[arg(long)]
        intent_id: String,

        /// Private key
        #[arg(long, env = "SOLVER_PRIVATE_KEY")]
        private_key: String,

        /// Dry-run mode
        #[arg(long)]
        dry_run: bool,
    },

    /// Test protocol adapter connectivity
    ///
    /// Example:
    /// $ taifoon test adapters
    /// $ taifoon test spinner-api
    Test {
        #[command(subcommand)]
        target: TestTarget,
    },

    /// Query solver performance stats
    ///
    /// Example:
    /// $ taifoon stats --since 24h
    /// $ taifoon stats --json
    Stats {
        /// Time window (e.g., "24h", "7d", "30d")
        #[arg(long, default_value = "24h")]
        since: String,
    },

    /// Show estimated fill costs per chain (gas × protocol) in wei, ETH, USDC, SOL
    ///
    /// Queries Razor/WARMBED gas API for real-time gas prices and computes
    /// the cost of a standard fill call for each supported protocol and chain.
    ///
    /// Example:
    /// $ taifoon fees
    /// $ taifoon fees --json
    Fees,

    /// Bootstrap a solver: generate/import key, register on Base Sepolia,
    /// write ~/.taifoon/solver.toml, print env-var snippets.
    ///
    /// Example:
    /// $ taifoon onboard
    /// $ taifoon onboard --import-key 0x... --registry-contract 0xCAFE...
    Onboard {
        /// Import an existing private key instead of generating one
        #[arg(long)]
        import_key: Option<String>,

        /// Registry contract address on Base Sepolia (omit for stub mode)
        #[arg(long)]
        registry_contract: Option<String>,

        /// Override the auto-derived solver_id
        #[arg(long)]
        solver_id: Option<String>,

        /// Overwrite existing ~/.taifoon/solver.toml if present
        #[arg(long)]
        force: bool,
    },

    /// Show multi-chain inventory: USDC/USDT/WETH balances + fill P&L
    ///
    /// Queries live RPC for Base, Optimism, and Arbitrum balances,
    /// and reports fill stats from the wallet-manager SQLite ledger.
    ///
    /// Example:
    /// $ taifoon portfolio --private-key 0x...
    /// $ taifoon portfolio --private-key 0x... --json
    Portfolio {
        /// Private key to derive solver address (mutually exclusive with --address)
        #[arg(long, env = "SOLVER_PRIVATE_KEY", conflicts_with = "address")]
        private_key: Option<String>,

        /// Solver address to inspect directly (no private key needed)
        #[arg(long, env = "SOLVER_ADDRESS")]
        address: Option<String>,

        /// Pull live data from the solver API instead of querying RPCs directly
        #[arg(long, env = "SOLVER_API_URL")]
        api_url: Option<String>,
    },
}

#[derive(Subcommand)]
enum WalletAction {
    /// Show wallet status (balance, address, network)
    Status {
        #[arg(long, env = "SOLVER_PRIVATE_KEY")]
        private_key: String,

        /// Chain ID to check balance on
        #[arg(long)]
        chain: Option<u64>,
    },

    /// Generate a new wallet
    Generate,

    /// Export wallet address from private key
    Address {
        #[arg(long, env = "SOLVER_PRIVATE_KEY")]
        private_key: String,
    },
}

#[derive(Subcommand)]
enum TestTarget {
    /// Test all protocol adapters
    Adapters,

    /// Test Spinner API connectivity
    SpinnerApi,

    /// Test Genome SSE stream
    GenomeStream,

    /// Run full end-to-end test (detect → estimate → build → simulate)
    E2e,
}

// ── Output Formatting ────────────────────────────────────────────────────────

#[derive(Serialize)]
struct JsonOutput<T> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T: Serialize> JsonOutput<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(msg: impl Into<String>) -> JsonOutput<()> {
        JsonOutput {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }

    fn print(&self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn print_json<T: Serialize>(data: T) {
    JsonOutput::success(data).print();
}

fn print_error_json(msg: impl Into<String>) {
    JsonOutput::<()>::error(msg).print();
}

fn print_human(msg: impl AsRef<str>) {
    println!("{}", msg.as_ref());
}

fn print_success(msg: impl AsRef<str>) {
    println!("{}", msg.as_ref().green().bold());
}

fn print_warn(msg: impl AsRef<str>) {
    println!("{}", msg.as_ref().yellow());
}

fn print_error(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref().red().bold());
}

// ── Main Entry Point ─────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging (suppress if --json mode)
    if !cli.json {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .init();
    }

    let result = match cli.command {
        Commands::Participate {
            private_key,
            auto,
            min_profit,
            protocol,
            dry_run,
            max_concurrent,
        } => {
            execute::participate(
                &cli.spinner_url,
                &cli.genome_url,
                &private_key,
                auto,
                min_profit,
                &protocol,
                dry_run,
                max_concurrent,
                cli.json,
            )
            .await
        }

        Commands::Wallet { action } => match action {
            WalletAction::Status { private_key, chain } => {
                wallet::status(&private_key, chain, &cli.spinner_url, cli.json).await
            }
            WalletAction::Generate => {
                wallet::generate(cli.json).await
            }
            WalletAction::Address { private_key } => {
                wallet::address(&private_key, cli.json).await
            }
        },

        Commands::Monitor {
            protocol,
            limit,
            profitable_only,
        } => {
            monitor::stream_intents(
                &cli.genome_url,
                &cli.spinner_url,
                protocol,
                limit,
                profitable_only,
                cli.json,
            )
            .await
        }

        Commands::Execute {
            intent_id,
            private_key,
            dry_run,
        } => {
            execute::single_fill(
                &cli.spinner_url,
                &intent_id,
                &private_key,
                dry_run,
                cli.json,
            )
            .await
        }

        Commands::Test { target } => match target {
            TestTarget::Adapters => test_mode::test_adapters(&cli.spinner_url, cli.json).await,
            TestTarget::SpinnerApi => test_mode::test_spinner(&cli.spinner_url, cli.json).await,
            TestTarget::GenomeStream => test_mode::test_genome(&cli.genome_url, cli.json).await,
            TestTarget::E2e => {
                test_mode::test_e2e(&cli.spinner_url, &cli.genome_url, cli.json).await
            }
        },

        Commands::Stats { since } => {
            monitor::stats(&since, &cli.spinner_url, cli.json).await
        }

        Commands::Fees => {
            commands::fees::run(&cli.spinner_url, cli.json).await
        }

        Commands::Onboard {
            import_key,
            registry_contract,
            solver_id,
            force,
        } => {
            commands::onboard::run(commands::onboard::OnboardArgs {
                import_key,
                registry_contract,
                solver_id,
                force,
                json_mode: cli.json,
            })
            .await
        }

        Commands::Portfolio { private_key, address, api_url } => {
            commands::portfolio::run(commands::portfolio::PortfolioArgs {
                private_key: private_key.unwrap_or_default(),
                address,
                api_url,
                json_mode: cli.json,
                spinner_url: cli.spinner_url,
            })
            .await
        }
    };

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            if cli.json {
                print_error_json(e.to_string());
            } else {
                print_error(format!("Error: {}", e));
            }
            std::process::exit(1);
        }
    }
}
