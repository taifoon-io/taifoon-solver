//! Generic Solana `simulateTransaction` runner + outcome classifier.
//!
//! Mirrors the shape of `executor::estimate::EstimateOutcome` so the executor
//! layer can collapse SVM and EVM outcomes into a single feed without losing
//! the SVM-specific signal (compute-unit consumption, lamport-shape failures).
//!
//! The classifier takes the *raw* `simulateTransaction` JSON-RPC response and
//! decides:
//!   - simulation succeeded with non-zero `unitsConsumed`            → OkComputeUnits
//!   - simulation succeeded but err present, log says insufficient   → InsufficientLamports
//!   - simulation failed with custom program error / invalid instr   → LogsContainError
//!   - request never reached the validator (network/parse error)     → InvalidIx
//!
//! The brief allowed either two new variants on the parent enum or a parallel
//! enum. We chose the parallel enum here — `executor` already has
//! `EstimateOutcome::OkComputeUnits` and `InsufficientLamports`, so we map the
//! SVM-specific variants up at the executor edge.

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolanaEstimateOutcome {
    /// `simulateTransaction` returned a positive `unitsConsumed` and no
    /// program-level error. The instruction decoded, all required accounts
    /// were resolved, and the program ran to completion. GREEN.
    OkComputeUnits(u64),
    /// Simulation surfaced an error that pattern-matches as a balance / fee /
    /// rent shortfall. The wallet is intentionally underfunded for the
    /// estimate phase; this is GREEN (calldata + ABI are correct).
    InsufficientLamports(String),
    /// Simulation reached the program but it returned a custom error or one
    /// of the standard `InstructionError` variants (e.g. `Custom`, `InvalidArgument`).
    /// This is RED: the program executed our instruction far enough to reject it.
    LogsContainError(String),
    /// The request never reached the validator, or the message couldn't be
    /// constructed (bad pubkey, oversized account list, JSON-RPC failure).
    /// This is RED — the Rust-side encoding is wrong.
    InvalidIx(String),
}

impl SolanaEstimateOutcome {
    pub fn is_green(&self) -> bool {
        matches!(
            self,
            SolanaEstimateOutcome::OkComputeUnits(_)
                | SolanaEstimateOutcome::InsufficientLamports(_)
        )
    }

    pub fn tag(&self) -> &'static str {
        match self {
            SolanaEstimateOutcome::OkComputeUnits(_) => "ok_compute_units",
            SolanaEstimateOutcome::InsufficientLamports(_) => "insufficient_lamports",
            SolanaEstimateOutcome::LogsContainError(_) => "logs_contain_error",
            SolanaEstimateOutcome::InvalidIx(_) => "invalid_ix",
        }
    }
}

/// Map a raw `simulateTransaction` JSON response (the inner `result.value`
/// object, *not* the JSON-RPC envelope) into one of the four outcomes.
///
/// The SVM emits a fixed vocabulary for funding-shaped failures: "insufficient
/// funds for rent", "Insufficient lamports", "AccountNotFound" (when the payer
/// hasn't been funded yet on a fresh keypair), or `InstructionError::Custom`
/// for program-level rejects. We classify on substrings rather than the full
/// `RpcSimulateTransactionResult` enum to stay independent of the
/// `solana-client` crate.
pub fn classify_solana_simulate_result(value: &serde_json::Value) -> SolanaEstimateOutcome {
    // `value.err` is `null` on success.
    let err = value.get("err");
    let units = value.get("unitsConsumed").and_then(|v| v.as_u64());
    let logs = value
        .get("logs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let logs_lower = logs.to_lowercase();

    // Successful simulation: err == null AND unitsConsumed > 0.
    let err_is_null = matches!(err, Some(serde_json::Value::Null) | None);
    if err_is_null {
        if let Some(u) = units {
            if u > 0 {
                return SolanaEstimateOutcome::OkComputeUnits(u);
            }
        }
        // err=null with no compute units is unusual but still arguably GREEN —
        // treat as OkComputeUnits(0) so it doesn't masquerade as a failure.
        return SolanaEstimateOutcome::OkComputeUnits(0);
    }

    let err_str = err.map(|e| e.to_string()).unwrap_or_default();
    let err_lower = err_str.to_lowercase();

    // Lamport / fee / rent shortfall — GREEN (wallet underfunded).
    // Substrings cover both the historic message variants and the structured
    // err shapes the validator emits ({"InstructionError":[0,"InsufficientFunds"]}
    // serialized as JSON contains the bare "insufficientfunds" token).
    if err_lower.contains("insufficient lamports")
        || err_lower.contains("insufficient funds")
        || err_lower.contains("insufficientfunds")
        || err_lower.contains("insufficientfundsforfee")
        || err_lower.contains("insufficient funds for rent")
        || logs_lower.contains("insufficient funds for rent")
        || logs_lower.contains("insufficient lamports")
        || logs_lower.contains("insufficient funds for fee")
        || logs_lower.contains("insufficient funds")
        || logs_lower.contains("account does not have enough sol")
    {
        return SolanaEstimateOutcome::InsufficientLamports(format!(
            "err={} logs={}",
            err_str,
            truncate(&logs, 256)
        ));
    }

    // The "AccountNotFound" failure we get with a fresh unfunded MESSIAH key
    // also signals a balance-shaped issue (the payer account doesn't exist on
    // mainnet because it has zero lamports and has never been the target of a
    // transfer). Treat that same way. `InvalidAccountForFee` is the
    // mainnet-beta variant emitted when the payer is a non-system account
    // (e.g. the System Program itself) — same shape: the fee/funding step
    // rejected, but the calldata reached the validator.
    if err_lower.contains("accountnotfound")
        || err_lower.contains("invalidaccountforfee")
    {
        return SolanaEstimateOutcome::InsufficientLamports(format!(
            "Payer-shape failure (fresh / unfunded / non-system payer): err={}",
            err_str
        ));
    }

    // Program-level reject (Custom errors, BorshIoError, InvalidAccountData,
    // ProgramFailedToComplete) — RED.
    if err_lower.contains("custom")
        || err_lower.contains("instructionerror")
        || err_lower.contains("programfailedtocomplete")
        || err_lower.contains("invalidaccountdata")
        || logs_lower.contains("custom program error")
        || logs_lower.contains("program failed")
    {
        return SolanaEstimateOutcome::LogsContainError(format!(
            "err={} logs={}",
            err_str,
            truncate(&logs, 512)
        ));
    }

    // Anything else with a non-null err falls through to InvalidIx — the
    // safer default than masking it as green.
    SolanaEstimateOutcome::InvalidIx(format!("err={}", err_str))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Thin wrapper around a Solana JSON-RPC endpoint used for `simulateTransaction`.
/// All requests go out with a 6-second timeout; longer than EVM because public
/// Solana RPCs are noisier.
#[derive(Debug, Clone)]
pub struct SolanaSimulator {
    pub rpc_url: String,
    client: reqwest::Client,
}

impl SolanaSimulator {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(6))
                .build()
                .expect("build reqwest client"),
        }
    }

    /// Fire a `simulateTransaction` JSON-RPC call against `rpc_url`. The base64
    /// blob must be a serialized legacy transaction. We always pass
    /// `sigVerify=false` and `replaceRecentBlockhash=true` so the validator
    /// doesn't reject on signature/blockhash issues — those aren't what we're
    /// trying to validate at the estimate stage.
    pub async fn simulate(&self, tx_b64: &str) -> SolanaEstimateOutcome {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "simulateTransaction",
            "params": [
                tx_b64,
                {
                    "sigVerify": false,
                    "replaceRecentBlockhash": true,
                    "encoding": "base64",
                    "commitment": "processed"
                }
            ]
        });

        let resp = match self
            .client
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return SolanaEstimateOutcome::InvalidIx(format!("rpc network error: {}", e));
            }
        };

        if !resp.status().is_success() {
            let st = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return SolanaEstimateOutcome::InvalidIx(format!(
                "rpc HTTP {}: {}",
                st,
                truncate(&body_text, 256)
            ));
        }

        let parsed: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                return SolanaEstimateOutcome::InvalidIx(format!("rpc body parse: {}", e));
            }
        };

        // JSON-RPC envelope: { jsonrpc, id, result: { context, value }, error? }
        if let Some(err) = parsed.get("error") {
            return SolanaEstimateOutcome::InvalidIx(format!("rpc error: {}", err));
        }
        let value = match parsed.pointer("/result/value") {
            Some(v) => v,
            None => {
                return SolanaEstimateOutcome::InvalidIx(
                    "rpc response missing result.value".to_string(),
                );
            }
        };

        classify_solana_simulate_result(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptBundleSolana {
    pub intent_id: String,
    pub protocol: String,
    pub outcome_tag: String,
    pub units_or_error: String,
    pub instruction_b64: String,
    pub payer_pubkey: String,
    pub program_id: String,
    pub ts: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ok_compute_units() {
        let v = serde_json::json!({
            "err": null,
            "unitsConsumed": 137_500,
            "logs": ["Program log: success"]
        });
        let out = classify_solana_simulate_result(&v);
        assert!(matches!(out, SolanaEstimateOutcome::OkComputeUnits(137_500)));
        assert!(out.is_green());
        assert_eq!(out.tag(), "ok_compute_units");
    }

    #[test]
    fn classifies_insufficient_lamports() {
        let v = serde_json::json!({
            "err": "InsufficientFundsForFee",
            "unitsConsumed": 0,
            "logs": []
        });
        let out = classify_solana_simulate_result(&v);
        assert!(matches!(out, SolanaEstimateOutcome::InsufficientLamports(_)));
        assert!(out.is_green());

        // Variant: err embedded in logs (some validators put it there)
        let v2 = serde_json::json!({
            "err": { "InstructionError": [0, "InsufficientFunds"] },
            "logs": ["Program log: insufficient lamports for transfer"]
        });
        let out2 = classify_solana_simulate_result(&v2);
        assert!(
            matches!(out2, SolanaEstimateOutcome::InsufficientLamports(_)),
            "got {:?}",
            out2
        );
    }

    #[test]
    fn classifies_logs_contain_error() {
        let v = serde_json::json!({
            "err": { "InstructionError": [0, { "Custom": 6001 }] },
            "unitsConsumed": 4500,
            "logs": [
                "Program BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY invoke [1]",
                "Program log: Custom program error: 0x1771",
                "Program BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY failed: custom program error"
            ]
        });
        let out = classify_solana_simulate_result(&v);
        assert!(
            matches!(out, SolanaEstimateOutcome::LogsContainError(_)),
            "got {:?}",
            out
        );
        assert!(!out.is_green());
        assert_eq!(out.tag(), "logs_contain_error");
    }

    #[test]
    fn classifies_invalid_ix_default() {
        // No matching pattern → InvalidIx
        let v = serde_json::json!({
            "err": "BlockhashNotFound",
            "logs": []
        });
        let out = classify_solana_simulate_result(&v);
        assert!(matches!(out, SolanaEstimateOutcome::InvalidIx(_)));
        assert!(!out.is_green());
        assert_eq!(out.tag(), "invalid_ix");
    }

    #[test]
    fn classifies_invalid_account_for_fee_as_lamport_shape() {
        // mainnet-beta returns this when the payer is a non-system account
        // (e.g. the System Program itself). It's a payer-shape rejection,
        // not an instruction-encoding bug.
        let v = serde_json::json!({
            "err": "InvalidAccountForFee",
            "logs": []
        });
        let out = classify_solana_simulate_result(&v);
        assert!(
            matches!(out, SolanaEstimateOutcome::InsufficientLamports(_)),
            "got {:?}",
            out
        );
        assert!(out.is_green(), "InvalidAccountForFee must be green");
    }

    #[test]
    fn classifies_account_not_found_as_lamport_shape() {
        // Fresh unfunded payer surfaces as AccountNotFound — treat as
        // InsufficientLamports because it's a balance-shaped issue, not an
        // ABI bug.
        let v = serde_json::json!({
            "err": "AccountNotFound",
            "logs": []
        });
        let out = classify_solana_simulate_result(&v);
        assert!(
            matches!(out, SolanaEstimateOutcome::InsufficientLamports(_)),
            "got {:?}",
            out
        );
    }
}
