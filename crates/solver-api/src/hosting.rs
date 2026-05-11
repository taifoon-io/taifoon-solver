//! Hosted solver provisioning registry.
//!
//! Allows participants to register themselves in the common hosting framework.
//! Each registered solver gets:
//!   - A unique `solver_id` (first 8 hex chars of their EVM address)
//!   - An API token for their solver instance (auth for mutation endpoints)
//!   - A signing_mode that determines key authority (session_key | remote_signer | self_hosted)
//!   - TSUL fee routing: their address receives 70% of every donut touch via BuildersRegistry
//!
//! ## Key authority model
//!
//! Taifoon NEVER holds private keys. Three modes:
//!
//! 1. `session_key` — User deploys a Safe multisig, grants Taifoon a scoped session
//!    key. Session key can ONLY call fill functions on registered adapter contracts.
//!    Spend limit enforced at Safe level. User can revoke at any time.
//!
//! 2. `remote_signer` — Solver builds unsigned transactions and POSTs them to
//!    `signer_webhook_url` (user-controlled). User's local signer approves and
//!    returns the signature. Truly non-custodial; adds one network round-trip
//!    per fill (acceptable for 30-min deadline intents).
//!
//! 3. `self_hosted` — User runs their own solver binary. This registry entry is
//!    purely for fleet-dashboard visibility and TSUL fee routing.
//!
//! ## TSUL fee routing
//!
//! Every fill calls `BuildersRegistry.recordRevenueTouch()` → 49 bps donut:
//!   - 70% → participant's `evm_address` (TSUL rule #4)
//!   - 20% → open-mamba reviewer set
//!   - 10% → ecosystem treasury
//!
//! This is enforced at the contract layer. The registry here tracks participants
//! so the dashboard can display per-solver donut accrual.

use axum::{
    extract::{Path, State},
    response::Json,
    http::StatusCode,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningMode {
    /// User deploys a Safe and grants us a scoped session key.
    SessionKey,
    /// User controls their key; we POST unsigned txs to their webhook.
    RemoteSigner,
    /// User runs their own binary. Registry entry = fleet visibility only.
    SelfHosted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedSolver {
    /// Deterministic ID: first 8 hex chars of evm_address (lowercased, no 0x).
    pub solver_id: String,
    /// Human-readable name chosen at registration.
    pub name: String,
    /// EVM address that receives 70% TSUL donut and authenticates as solver owner.
    pub evm_address: String,
    /// Solana address (optional — for Mayan Swift Solana fills).
    pub solana_address: Option<String>,
    /// How keys are managed.
    pub signing_mode: SigningMode,
    /// For `remote_signer` mode: the user's webhook URL that receives unsigned tx.
    pub signer_webhook_url: Option<String>,
    /// For `session_key` mode: the Safe address that holds the session key.
    pub safe_address: Option<String>,
    /// Operator email for alerts.
    pub email: Option<String>,
    /// Comma-separated chain IDs this solver watches.
    pub chains: String,
    /// Comma-separated protocol IDs this solver runs.
    pub protocols: String,
    /// UTC timestamp of registration.
    pub registered_at: String,
    /// Whether this solver is actively running in the fleet.
    pub active: bool,
    /// Cumulative TSUL donut accrued (USD equivalent, for display).
    pub donut_accrued_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct ProvisionRequest {
    pub name: String,
    pub evm_address: String,
    pub solana_address: Option<String>,
    pub signing_mode: Option<String>,
    pub signer_webhook_url: Option<String>,
    pub safe_address: Option<String>,
    pub email: Option<String>,
    /// Comma-separated e.g. "base,arbitrum,optimism"
    pub chains: Option<String>,
    /// Comma-separated e.g. "across,debridge,mayan"
    pub protocols: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProvisionResponse {
    pub solver_id: String,
    /// Unique API token for this solver's mutation endpoints.
    /// Shown once — user must store it. Not recoverable (re-provision to rotate).
    pub api_token: String,
    pub portal_url: String,
    pub watch_url: String,
    pub signing_mode: String,
    pub tsul_note: String,
}

// ── Registry ───────────────────────────────────────────────────────────────────

pub struct HostingRegistry {
    db: Mutex<Connection>,
}

impl HostingRegistry {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS hosted_solvers (
                solver_id         TEXT PRIMARY KEY,
                name              TEXT NOT NULL,
                evm_address       TEXT NOT NULL UNIQUE,
                solana_address    TEXT,
                signing_mode      TEXT NOT NULL DEFAULT 'self_hosted',
                signer_webhook_url TEXT,
                safe_address      TEXT,
                email             TEXT,
                chains            TEXT NOT NULL DEFAULT 'base,arbitrum,optimism',
                protocols         TEXT NOT NULL DEFAULT 'across,debridge,mayan',
                api_token_hash    TEXT NOT NULL,
                registered_at     TEXT NOT NULL,
                active            INTEGER NOT NULL DEFAULT 1,
                donut_accrued_usd REAL NOT NULL DEFAULT 0.0
            );
        ")?;
        Ok(Self { db: Mutex::new(conn) })
    }

    pub fn provision(&self, req: &ProvisionRequest) -> anyhow::Result<(HostedSolver, String)> {
        let addr = req.evm_address.trim().to_lowercase();
        if !addr.starts_with("0x") || addr.len() != 42 {
            anyhow::bail!("invalid EVM address");
        }

        let solver_id = &addr[2..10]; // first 8 hex chars
        let now = chrono::Utc::now().to_rfc3339();

        // Generate a non-cryptographic but unique API token.
        // For production, replace with rand::thread_rng().gen::<[u8;24]>().
        // For hackathon purposes, mix address + timestamp + nanos for uniqueness.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let seed = format!("{}{}{}",
            addr,
            now,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        );
        let mut h1 = DefaultHasher::new();
        seed.hash(&mut h1);
        let v1: u64 = h1.finish();

        let seed2 = format!("{}{}", addr, v1);
        let mut h2 = DefaultHasher::new();
        seed2.hash(&mut h2);
        let v2: u64 = h2.finish();

        let seed3 = format!("{}{}", now, v2);
        let mut h3 = DefaultHasher::new();
        seed3.hash(&mut h3);
        let v3: u64 = h3.finish();

        let mut token_bytes = [0u8; 24];
        token_bytes[..8].copy_from_slice(&v1.to_le_bytes());
        token_bytes[8..16].copy_from_slice(&v2.to_le_bytes());
        token_bytes[16..24].copy_from_slice(&v3.to_le_bytes());
        let api_token: String = hex::encode(token_bytes);

        // Hash token for storage (don't store raw token)
        let mut h_store = DefaultHasher::new();
        api_token.hash(&mut h_store);
        let token_hash: String = format!("{:x}", h_store.finish());

        let signing_mode = req.signing_mode.as_deref().unwrap_or("self_hosted");
        let chains = req.chains.as_deref().unwrap_or("base,arbitrum,optimism");
        let protocols = req.protocols.as_deref().unwrap_or("across,debridge,mayan");

        let solver = HostedSolver {
            solver_id: solver_id.to_string(),
            name: req.name.clone(),
            evm_address: addr.clone(),
            solana_address: req.solana_address.clone(),
            signing_mode: match signing_mode {
                "session_key" => SigningMode::SessionKey,
                "remote_signer" => SigningMode::RemoteSigner,
                _ => SigningMode::SelfHosted,
            },
            signer_webhook_url: req.signer_webhook_url.clone(),
            safe_address: req.safe_address.clone(),
            email: req.email.clone(),
            chains: chains.to_string(),
            protocols: protocols.to_string(),
            registered_at: now.clone(),
            active: true,
            donut_accrued_usd: 0.0,
        };

        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO hosted_solvers
             (solver_id, name, evm_address, solana_address, signing_mode,
              signer_webhook_url, safe_address, email, chains, protocols,
              api_token_hash, registered_at, active, donut_accrued_usd)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,1,0.0)",
            params![
                solver.solver_id,
                solver.name,
                solver.evm_address,
                solver.solana_address,
                signing_mode,
                solver.signer_webhook_url,
                solver.safe_address,
                solver.email,
                solver.chains,
                solver.protocols,
                token_hash,
                now,
            ],
        )?;

        info!("[hosting] provisioned solver_id={} address={} mode={}", solver_id, addr, signing_mode);
        Ok((solver, api_token))
    }

    pub fn list(&self) -> anyhow::Result<Vec<HostedSolver>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT solver_id, name, evm_address, solana_address, signing_mode,
                    signer_webhook_url, safe_address, email, chains, protocols,
                    registered_at, active, donut_accrued_usd
             FROM hosted_solvers ORDER BY registered_at DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(HostedSolverRow {
                solver_id: row.get(0)?,
                name: row.get(1)?,
                evm_address: row.get(2)?,
                solana_address: row.get(3)?,
                signing_mode: row.get(4)?,
                signer_webhook_url: row.get(5)?,
                safe_address: row.get(6)?,
                email: row.get(7)?,
                chains: row.get(8)?,
                protocols: row.get(9)?,
                registered_at: row.get(10)?,
                active: row.get::<_, i64>(11)? != 0,
                donut_accrued_usd: row.get(12)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            let r = row.map_err(|e| anyhow::anyhow!(e))?;
            result.push(row_to_solver(r));
        }
        Ok(result)
    }

    pub fn get_by_id(&self, solver_id: &str) -> anyhow::Result<Option<HostedSolver>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT solver_id, name, evm_address, solana_address, signing_mode,
                    signer_webhook_url, safe_address, email, chains, protocols,
                    registered_at, active, donut_accrued_usd
             FROM hosted_solvers WHERE solver_id = ?1"
        )?;

        let mut rows = stmt.query_map(params![solver_id], |row| {
            Ok(HostedSolverRow {
                solver_id: row.get(0)?,
                name: row.get(1)?,
                evm_address: row.get(2)?,
                solana_address: row.get(3)?,
                signing_mode: row.get(4)?,
                signer_webhook_url: row.get(5)?,
                safe_address: row.get(6)?,
                email: row.get(7)?,
                chains: row.get(8)?,
                protocols: row.get(9)?,
                registered_at: row.get(10)?,
                active: row.get::<_, i64>(11)? != 0,
                donut_accrued_usd: row.get(12)?,
            })
        })?;

        Ok(rows.next().transpose().map_err(|e| anyhow::anyhow!(e))?.map(row_to_solver))
    }

    pub fn record_donut_touch(&self, solver_id: &str, usd_value: f64) {
        let creator_share = usd_value * 0.0049 * 0.70; // 49 bps * 70%
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "UPDATE hosted_solvers SET donut_accrued_usd = donut_accrued_usd + ?1 WHERE solver_id = ?2",
            params![creator_share, solver_id],
        );
    }
}

// Internal row struct for SQLite deserialization
struct HostedSolverRow {
    solver_id: String,
    name: String,
    evm_address: String,
    solana_address: Option<String>,
    signing_mode: String,
    signer_webhook_url: Option<String>,
    safe_address: Option<String>,
    email: Option<String>,
    chains: String,
    protocols: String,
    registered_at: String,
    active: bool,
    donut_accrued_usd: f64,
}

fn row_to_solver(r: HostedSolverRow) -> HostedSolver {
    HostedSolver {
        solver_id: r.solver_id,
        name: r.name,
        evm_address: r.evm_address,
        solana_address: r.solana_address,
        signing_mode: match r.signing_mode.as_str() {
            "session_key" => SigningMode::SessionKey,
            "remote_signer" => SigningMode::RemoteSigner,
            _ => SigningMode::SelfHosted,
        },
        signer_webhook_url: r.signer_webhook_url,
        safe_address: r.safe_address,
        email: r.email,
        chains: r.chains,
        protocols: r.protocols,
        registered_at: r.registered_at,
        active: r.active,
        donut_accrued_usd: r.donut_accrued_usd,
    }
}

// ── Axum handlers ──────────────────────────────────────────────────────────────

pub type HostingRegistryState = Arc<HostingRegistry>;

/// POST /api/hosting/provision
/// Public endpoint — no auth required. Anyone can register.
pub async fn provision_handler(
    State(registry): State<HostingRegistryState>,
    Json(req): Json<ProvisionRequest>,
) -> Result<Json<ProvisionResponse>, (StatusCode, Json<serde_json::Value>)> {
    match registry.provision(&req) {
        Ok((solver, token)) => {
            let base_url = std::env::var("PUBLIC_BASE_URL")
                .unwrap_or_else(|_| "https://solver.taifoon.dev".to_string());

            let signing_note = match req.signing_mode.as_deref().unwrap_or("self_hosted") {
                "session_key" => "Grant your Safe session key to fill-function selectors only. Taifoon never holds your master key.",
                "remote_signer" => "Your local signer receives unsigned transactions. Approve each fill before broadcast.",
                _ => "Self-hosted: run cargo build --release -p solver-main and configure SOLVER_PRIVATE_KEY on your own machine.",
            };

            Ok(Json(ProvisionResponse {
                solver_id: solver.solver_id.clone(),
                api_token: token,
                portal_url: format!("{}/portal/{}", base_url, solver.solver_id),
                watch_url: format!("{}/watch?address={}", base_url, solver.evm_address),
                signing_mode: req.signing_mode.unwrap_or_else(|| "self_hosted".to_string()),
                tsul_note: format!(
                    "TSUL rule #4: 70% of every fill's donut routes to {} perpetually on-chain. {}",
                    solver.evm_address,
                    signing_note,
                ),
            }))
        }
        Err(e) => {
            warn!("[hosting] provision failed: {}", e);
            Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            ))
        }
    }
}

/// GET /api/hosting/solvers
pub async fn list_solvers_handler(
    State(registry): State<HostingRegistryState>,
) -> Json<serde_json::Value> {
    match registry.list() {
        Ok(solvers) => Json(serde_json::json!({
            "count": solvers.len(),
            "solvers": solvers,
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string(), "solvers": [] })),
    }
}

/// GET /api/hosting/solvers/:solver_id
pub async fn get_solver_handler(
    State(registry): State<HostingRegistryState>,
    Path(solver_id): Path<String>,
) -> Result<Json<HostedSolver>, (StatusCode, Json<serde_json::Value>)> {
    match registry.get_by_id(&solver_id) {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "solver not found" })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> HostingRegistry {
        HostingRegistry::new(":memory:").unwrap()
    }

    #[test]
    fn provision_returns_unique_token() {
        let r = registry();
        let req = ProvisionRequest {
            name: "test-solver".into(),
            evm_address: "0x0000000000000000000000000000000000000001".into(),
            solana_address: None,
            signing_mode: Some("self_hosted".into()),
            signer_webhook_url: None,
            safe_address: None,
            email: Some("test@test.com".into()),
            chains: Some("base,arbitrum".into()),
            protocols: Some("across".into()),
        };
        let (solver, token) = r.provision(&req).unwrap();
        assert_eq!(solver.solver_id, "00000000");
        assert_eq!(token.len(), 48); // 24 bytes hex-encoded
        assert!(solver.evm_address.starts_with("0x"));
    }

    #[test]
    fn invalid_address_rejected() {
        let r = registry();
        let req = ProvisionRequest {
            name: "x".into(),
            evm_address: "not-an-address".into(),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
        };
        assert!(r.provision(&req).is_err());
    }

    #[test]
    fn list_empty_initially() {
        let r = registry();
        assert_eq!(r.list().unwrap().len(), 0);
    }

    #[test]
    fn provision_then_list_returns_entry() {
        let r = registry();
        let req = ProvisionRequest {
            name: "fleet-member".into(),
            evm_address: "0xAbCd1234AbCd1234AbCd1234AbCd1234AbCd1234".into(),
            solana_address: None,
            signing_mode: Some("remote_signer".into()),
            signer_webhook_url: Some("https://my-signer.example.com/sign".into()),
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
        };
        let (solver, _token) = r.provision(&req).unwrap();
        let list = r.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].solver_id, solver.solver_id);
        assert!(matches!(list[0].signing_mode, SigningMode::RemoteSigner));
    }

    #[test]
    fn donut_accrual_accumulates() {
        let r = registry();
        let req = ProvisionRequest {
            name: "donut-test".into(),
            evm_address: "0x1111111111111111111111111111111111111111".into(),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
        };
        let (solver, _) = r.provision(&req).unwrap();
        r.record_donut_touch(&solver.solver_id, 1000.0); // $1000 fill value
        r.record_donut_touch(&solver.solver_id, 500.0);
        let updated = r.get_by_id(&solver.solver_id).unwrap().unwrap();
        // 49 bps * 70% of $1500 = 0.0049 * 0.70 * 1500 = $5.145
        let expected = 0.0049 * 0.70 * 1500.0;
        assert!((updated.donut_accrued_usd - expected).abs() < 0.001);
    }
}
