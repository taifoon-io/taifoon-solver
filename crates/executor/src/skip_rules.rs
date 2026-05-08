//! Skip-rule cache (X1: self-learning loop, consumer side).
//!
//! At startup the solver fetches `GET {mamba}/api/solver/skip-rules` and
//! caches the active rule set in memory. Each rule is a JSON predicate
//! authored by the weekly nemotron analyzer. The canonical shape is:
//!
//! ```json
//! { "min_amount_usd": 200.0, "max_gas_gwei": 50.0, "dst_chain": 42161 }
//! ```
//!
//! Semantics: an intent is skipped iff *every* present field matches.
//!   - `min_amount_usd`:        skip when intent_amount_usd < min_amount_usd
//!   - `max_gas_gwei`:          skip when current dst-gas_gwei > max_gas_gwei
//!   - `dst_chain`:             skip only on this dst chain (omitted = all chains)
//!   - `src_chain`:             skip only on this src chain (omitted = all chains)
//!   - `max_fill_deadline_secs`: skip when fill_deadline is > now + max_fill_deadline_secs
//!                               (i.e. deadline is too far in the future — stale/suspicious order)
//!
//! All fields are optional, but at least one must be present (a
//! rule with no predicates would skip everything — the analyzer must
//! never publish that, and we defensively reject it on load).

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Deserialize)]
struct WireRule {
    rule_json: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    protocol: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RulePredicate {
    #[serde(default)]
    pub min_amount_usd: Option<f64>,
    #[serde(default)]
    pub max_gas_gwei: Option<f64>,
    #[serde(default)]
    pub dst_chain: Option<u64>,
    #[serde(default)]
    pub src_chain: Option<u64>,
    /// Skip when fill_deadline > now + max_fill_deadline_secs (order expires too far out).
    #[serde(default)]
    pub max_fill_deadline_secs: Option<u64>,
}

impl RulePredicate {
    fn is_useful(&self) -> bool {
        self.min_amount_usd.is_some()
            || self.max_gas_gwei.is_some()
            || self.dst_chain.is_some()
            || self.src_chain.is_some()
            || self.max_fill_deadline_secs.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct SkipRule {
    pub protocol: String,
    pub predicate: RulePredicate,
    pub description: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SkipRules {
    rules: Vec<SkipRule>,
}

impl SkipRules {
    pub fn empty() -> Self { Self::default() }

    pub fn len(&self) -> usize { self.rules.len() }
    pub fn is_empty(&self) -> bool { self.rules.is_empty() }

    /// Fetch active rules from mamba. Returns `Self::empty()` and logs a
    /// warning on any failure — startup must never block on the lake.
    pub async fn fetch(mamba_url: &str) -> Self {
        let url = format!("{}/api/solver/skip-rules", mamba_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Vec<WireRule>>().await {
                    Ok(wire) => Self::from_wire(wire),
                    Err(e) => { warn!("skip-rules: bad json from {}: {}", url, e); Self::empty() }
                }
            }
            Ok(resp) => { warn!("skip-rules: GET {} -> {}", url, resp.status()); Self::empty() }
            Err(e)   => { warn!("skip-rules: GET {} failed: {}", url, e); Self::empty() }
        }
    }

    fn from_wire(wire: Vec<WireRule>) -> Self {
        let mut rules = Vec::with_capacity(wire.len());
        for w in wire {
            let pred: RulePredicate = match serde_json::from_str(&w.rule_json) {
                Ok(p) => p,
                Err(e) => { warn!("skip-rules: drop unparseable rule ({}): {}", w.protocol, e); continue; }
            };
            if !pred.is_useful() {
                warn!("skip-rules: drop empty predicate for protocol {}", w.protocol);
                continue;
            }
            rules.push(SkipRule {
                protocol: w.protocol,
                predicate: pred,
                description: w.description,
                confidence: w.confidence,
            });
        }
        // Warn about max_gas_gwei predicates: the current call site in solver-main
        // always passes current_gas_gwei=None (no RPC at the skip-rule fast-path).
        // Rules that rely solely on max_gas_gwei will never fire until a gas oracle
        // is wired into the evaluate call. Surface this at load time so operators
        // don't wonder why gas-based rules are inactive.
        let gas_only_rules = rules.iter()
            .filter(|r| r.predicate.max_gas_gwei.is_some()
                && r.predicate.min_amount_usd.is_none()
                && r.predicate.dst_chain.is_none()
                && r.predicate.src_chain.is_none()
                && r.predicate.max_fill_deadline_secs.is_none())
            .count();
        if gas_only_rules > 0 {
            warn!(
                "skip-rules: {} rule(s) use only max_gas_gwei — these will NOT fire \
                 because current_gas_gwei is not yet supplied at the evaluate call site",
                gas_only_rules
            );
        }
        info!("📐 skip-rules: loaded {} active rules", rules.len());
        Self { rules }
    }

    /// Returns `Some(reason_string)` when an active rule fires.
    /// `intent_amount_usd` and `current_gas_gwei` are caller-supplied —
    /// the executor already has both from its pre-flight gas estimate.
    pub fn evaluate(
        &self,
        protocol: &str,
        src_chain: u64,
        dst_chain: u64,
        intent_amount_usd: Option<f64>,
        current_gas_gwei: Option<f64>,
        fill_deadline: Option<u32>,
    ) -> Option<String> {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let p = protocol.to_lowercase();
        for r in &self.rules {
            if !r.protocol.is_empty() && r.protocol.to_lowercase() != p {
                continue;
            }
            if let Some(c) = r.predicate.dst_chain { if c != dst_chain { continue; } }
            if let Some(c) = r.predicate.src_chain { if c != src_chain { continue; } }
            if let Some(min) = r.predicate.min_amount_usd {
                let Some(amt) = intent_amount_usd else { continue; };
                if amt >= min { continue; }
            }
            if let Some(max) = r.predicate.max_gas_gwei {
                let Some(g) = current_gas_gwei else { continue; };
                if g <= max { continue; }
            }
            if let Some(max_deadline_secs) = r.predicate.max_fill_deadline_secs {
                let Some(dl) = fill_deadline else { continue; };
                // Skip if deadline is more than max_deadline_secs in the future.
                if (dl as u64) <= now_secs + max_deadline_secs { continue; }
            }
            // All present predicates matched — fire.
            let reason = r.description.clone().unwrap_or_else(|| {
                format!(
                    "skip_rule:src={:?},dst={:?},min_amount_usd={:?},max_gas_gwei={:?},max_fill_deadline_secs={:?}",
                    r.predicate.src_chain, r.predicate.dst_chain,
                    r.predicate.min_amount_usd, r.predicate.max_gas_gwei,
                    r.predicate.max_fill_deadline_secs,
                )
            });
            return Some(reason);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(json: &str, protocol: &str) -> WireRule {
        WireRule { rule_json: json.to_string(), description: None, confidence: 0.9, protocol: protocol.to_string() }
    }

    #[test]
    fn empty_predicate_dropped() {
        let r = SkipRules::from_wire(vec![rule("{}", "across")]);
        assert!(r.is_empty());
    }

    #[test]
    fn fires_when_all_present_predicates_match() {
        let r = SkipRules::from_wire(vec![rule(
            r#"{"min_amount_usd":200.0,"max_gas_gwei":50.0}"#, "across",
        )]);
        // amount below 200 AND gas above 50 → skip
        assert!(r.evaluate("across", 1, 8453, Some(150.0), Some(60.0), None).is_some());
        // amount above 200 → keep
        assert!(r.evaluate("across", 1, 8453, Some(250.0), Some(60.0), None).is_none());
        // gas below 50 → keep
        assert!(r.evaluate("across", 1, 8453, Some(150.0), Some(40.0), None).is_none());
    }

    #[test]
    fn protocol_isolation() {
        let r = SkipRules::from_wire(vec![rule(
            r#"{"min_amount_usd":200.0}"#, "across",
        )]);
        assert!(r.evaluate("debridge", 1, 8453, Some(50.0), None, None).is_none());
        assert!(r.evaluate("across",   1, 8453, Some(50.0), None, None).is_some());
    }

    #[test]
    fn dst_chain_filter() {
        let r = SkipRules::from_wire(vec![rule(
            r#"{"min_amount_usd":200.0,"dst_chain":42161}"#, "across",
        )]);
        assert!(r.evaluate("across", 1, 1,     Some(50.0), None, None).is_none());
        assert!(r.evaluate("across", 1, 42161, Some(50.0), None, None).is_some());
    }

    #[test]
    fn src_chain_filter() {
        let r = SkipRules::from_wire(vec![rule(
            r#"{"min_amount_usd":100.0,"src_chain":137}"#, "across",
        )]);
        // src_chain matches and amount below min → skip
        assert!(r.evaluate("across", 137, 8453, Some(50.0), None, None).is_some());
        // src_chain doesn't match → keep
        assert!(r.evaluate("across", 1,   8453, Some(50.0), None, None).is_none());
    }

    #[test]
    fn fill_deadline_filter() {
        let r = SkipRules::from_wire(vec![rule(
            r#"{"max_fill_deadline_secs":3600}"#, "across",
        )]);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // deadline 2 hours in the future → skip (> now + 3600)
        let far_deadline = (now + 7200) as u32;
        assert!(r.evaluate("across", 1, 8453, None, None, Some(far_deadline)).is_some());
        // deadline 30 min in the future → keep (< now + 3600)
        let near_deadline = (now + 1800) as u32;
        assert!(r.evaluate("across", 1, 8453, None, None, Some(near_deadline)).is_none());
        // no fill_deadline provided → keep (can't evaluate, predicate requires it)
        assert!(r.evaluate("across", 1, 8453, None, None, None).is_none());
    }
}
