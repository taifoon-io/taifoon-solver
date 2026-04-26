//! Automated Fixture Validator
//!
//! Validates all 104+ intent fixtures against the Razor API with real gas prices.
//! Catches all edge cases: >$1 profits, None values, 0 values, unrealistic costs.
//!
//! Usage:
//!   cargo test --test fixture_validator -- --nocapture

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Chain gas price from Razor API
#[derive(Debug, Clone, Deserialize)]
struct RazorGasPreset {
    chain_id: u64,
    chain_name: String,
    ready: bool,
    gas_cost_usd: Option<f64>,
    gas_cost_gwei: Option<f64>,
    gas_price_gwei: Option<f64>,
    reason: Option<String>,
}

/// Profit validation result
#[derive(Debug, Clone)]
struct ProfitValidation {
    intent_id: String,
    protocol: String,
    src_chain: u64,
    dst_chain: u64,
    raw_profit_usd: Option<f64>,
    recalculated_profit_usd: Option<f64>,
    gas_cost_usd: Option<f64>,
    src_gas_gwei: Option<f64>,
    dst_gas_gwei: Option<f64>,
    issues: Vec<String>,
    status: ValidationStatus,
}

#[derive(Debug, Clone, PartialEq)]
enum ValidationStatus {
    Pass,
    Fail,
    Warning,
}

/// Test fixture intent
#[derive(Debug, Clone, Deserialize)]
struct FixtureIntent {
    id: Option<String>,
    protocol: Option<String>,
    src_chain: Option<u64>,
    dst_chain: Option<u64>,
    amount: Option<String>,
    token: Option<String>,
    depositor: Option<String>,
    recipient: Option<String>,
    profit_usd: Option<f64>,

    // Protocol-specific fields
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

/// Test report
#[derive(Debug)]
struct TestReport {
    total_intents: usize,
    passed: usize,
    failed: usize,
    warnings: usize,
    validations: Vec<ProfitValidation>,
}

impl TestReport {
    fn new() -> Self {
        Self {
            total_intents: 0,
            passed: 0,
            failed: 0,
            warnings: 0,
            validations: Vec::new(),
        }
    }

    fn add(&mut self, validation: ProfitValidation) {
        match validation.status {
            ValidationStatus::Pass => self.passed += 1,
            ValidationStatus::Fail => self.failed += 1,
            ValidationStatus::Warning => self.warnings += 1,
        }
        self.total_intents += 1;
        self.validations.push(validation);
    }

    fn print_summary(&self) {
        println!("\n{'='}==========================================================================");
        println!("TEST SUMMARY");
        println!("{'='}==========================================================================");
        println!("Total Intents:  {}", self.total_intents);
        println!("✅ PASS:        {} ({:.1}%)", self.passed,
            (self.passed as f64 / self.total_intents as f64) * 100.0);
        println!("❌ FAIL:        {} ({:.1}%)", self.failed,
            (self.failed as f64 / self.total_intents as f64) * 100.0);
        println!("⚠️  WARNING:    {} ({:.1}%)", self.warnings,
            (self.warnings as f64 / self.total_intents as f64) * 100.0);
        println!("{'='}==========================================================================\n");
    }

    fn print_failures(&self) {
        let failures: Vec<_> = self.validations.iter()
            .filter(|v| v.status == ValidationStatus::Fail)
            .collect();

        if failures.is_empty() {
            return;
        }

        println!("\n{'='}==========================================================================");
        println!("FAILURES ({})", failures.len());
        println!("{'='}==========================================================================");

        for v in failures {
            println!("\n❌ {}", v.intent_id);
            println!("   Protocol: {}", v.protocol);
            println!("   Route: {} → {}", v.src_chain, v.dst_chain);

            if let Some(profit) = v.raw_profit_usd {
                println!("   Raw Profit: ${:.6}", profit);
            }
            if let Some(profit) = v.recalculated_profit_usd {
                println!("   Recalculated Profit: ${:.6}", profit);
            }
            if let Some(gas) = v.gas_cost_usd {
                println!("   Gas Cost: ${:.6}", gas);
            }
            if let Some(gwei) = v.src_gas_gwei {
                println!("   Src Gas: {} gwei", gwei);
            }
            if let Some(gwei) = v.dst_gas_gwei {
                println!("   Dst Gas: {} gwei", gwei);
            }

            println!("   Issues:");
            for issue in &v.issues {
                println!("     • {}", issue);
            }
        }
        println!();
    }

    fn print_warnings(&self) {
        let warnings: Vec<_> = self.validations.iter()
            .filter(|v| v.status == ValidationStatus::Warning)
            .collect();

        if warnings.is_empty() {
            return;
        }

        println!("\n{'='}==========================================================================");
        println!("WARNINGS ({})", warnings.len());
        println!("{'='}==========================================================================");

        for v in warnings {
            println!("\n⚠️  {}", v.intent_id);
            println!("   Protocol: {}", v.protocol);
            println!("   Route: {} → {}", v.src_chain, v.dst_chain);

            if let Some(profit) = v.raw_profit_usd {
                println!("   Raw Profit: ${:.6}", profit);
            }

            println!("   Issues:");
            for issue in &v.issues {
                println!("     • {}", issue);
            }
        }
        println!();
    }

    fn protocol_breakdown(&self) {
        let mut by_protocol: HashMap<String, (usize, usize, usize)> = HashMap::new();

        for v in &self.validations {
            let entry = by_protocol.entry(v.protocol.clone()).or_insert((0, 0, 0));
            match v.status {
                ValidationStatus::Pass => entry.0 += 1,
                ValidationStatus::Fail => entry.1 += 1,
                ValidationStatus::Warning => entry.2 += 1,
            }
        }

        println!("\n{'='}==========================================================================");
        println!("PROTOCOL BREAKDOWN");
        println!("{'='}==========================================================================");
        println!("{:<20} {:>8} {:>8} {:>8} {:>10}", "Protocol", "Pass", "Fail", "Warn", "Pass Rate");
        println!("{:-<20} {:->8} {:->8} {:->8} {:->10}", "", "", "", "", "");

        let mut protocols: Vec<_> = by_protocol.iter().collect();
        protocols.sort_by_key(|(name, _)| *name);

        for (protocol, (pass, fail, warn)) in protocols {
            let total = pass + fail + warn;
            let pass_rate = (*pass as f64 / total as f64) * 100.0;
            println!("{:<20} {:>8} {:>8} {:>8} {:>9.1}%",
                protocol, pass, fail, warn, pass_rate);
        }
        println!();
    }
}

/// Fetch gas prices from Razor API
async fn fetch_razor_gas_prices() -> Result<HashMap<u64, RazorGasPreset>, Box<dyn std::error::Error>> {
    let url = "http://localhost:9081/api/solver/razor";

    let response = reqwest::get(url).await?;
    let body: Value = response.json().await?;

    let presets: Vec<RazorGasPreset> = serde_json::from_value(
        body.get("presets")
            .ok_or("Missing 'presets' field")?
            .clone()
    )?;

    let mut map = HashMap::new();
    for preset in presets {
        map.insert(preset.chain_id, preset);
    }

    Ok(map)
}

/// Load intent fixtures from a JSON file
fn load_fixtures(path: &Path) -> Result<Vec<FixtureIntent>, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)?;
    let intents: Vec<FixtureIntent> = serde_json::from_str(&contents)?;
    Ok(intents)
}

/// Validate a single intent
fn validate_intent(
    intent: &FixtureIntent,
    gas_prices: &HashMap<u64, RazorGasPreset>,
) -> ProfitValidation {
    let intent_id = intent.id.clone().unwrap_or_else(|| "unknown".to_string());
    let protocol = intent.protocol.clone().unwrap_or_else(|| "unknown".to_string());
    let src_chain = intent.src_chain.unwrap_or(0);
    let dst_chain = intent.dst_chain.unwrap_or(0);

    let mut issues = Vec::new();
    let mut status = ValidationStatus::Pass;

    // Get gas prices
    let src_gas = gas_prices.get(&src_chain);
    let dst_gas = gas_prices.get(&dst_chain);

    let src_gas_gwei = src_gas.and_then(|g| g.gas_price_gwei.or(g.gas_cost_gwei));
    let dst_gas_gwei = dst_gas.and_then(|g| g.gas_price_gwei.or(g.gas_cost_gwei));

    // Check for missing gas prices
    if src_gas.is_none() {
        issues.push(format!("Missing gas price for src chain {}", src_chain));
        status = ValidationStatus::Warning;
    }
    if dst_gas.is_none() {
        issues.push(format!("Missing gas price for dst chain {}", dst_chain));
        status = ValidationStatus::Warning;
    }

    // Check for zero gas prices
    if let Some(gwei) = src_gas_gwei {
        if gwei == 0.0 {
            issues.push(format!("Zero gas price for src chain {}", src_chain));
            status = ValidationStatus::Fail;
        }
    }
    if let Some(gwei) = dst_gas_gwei {
        if gwei == 0.0 {
            issues.push(format!("Zero gas price for dst chain {}", dst_chain));
            status = ValidationStatus::Fail;
        }
    }

    // Check for None gas_cost_usd
    let src_gas_cost_usd = src_gas.and_then(|g| g.gas_cost_usd);
    let dst_gas_cost_usd = dst_gas.and_then(|g| g.gas_cost_usd);

    if src_gas_cost_usd.is_none() {
        issues.push(format!("Missing gas_cost_usd for src chain {}", src_chain));
        status = ValidationStatus::Warning;
    }
    if dst_gas_cost_usd.is_none() {
        issues.push(format!("Missing gas_cost_usd for dst chain {}", dst_chain));
        status = ValidationStatus::Warning;
    }

    // Calculate total gas cost
    let total_gas_cost_usd = match (src_gas_cost_usd, dst_gas_cost_usd) {
        (Some(src), Some(dst)) => Some(src + dst),
        _ => None,
    };

    // Check raw profit from fixture
    let raw_profit = intent.profit_usd;
    if let Some(profit) = raw_profit {
        if profit > 1.0 {
            issues.push(format!("Unrealistic profit: ${:.2} (should be < $1)", profit));
            status = ValidationStatus::Fail;
        }
        if profit < -2.0 {
            issues.push(format!("Unrealistic loss: ${:.2} (should be > -$2)", profit));
            status = ValidationStatus::Fail;
        }
    }

    // Estimate recalculated profit (very rough estimate)
    // Real calculation would need amount, token price, protocol fees, etc.
    let recalculated_profit = if let Some(gas_cost) = total_gas_cost_usd {
        // Assume zero protocol fees and zero spread for simplicity
        // Real profit = amount_value - gas_cost - protocol_fees
        Some(-gas_cost) // Pessimistic: only gas cost, no revenue
    } else {
        None
    };

    // Check if gas cost alone is > $1 (red flag)
    if let Some(gas_cost) = total_gas_cost_usd {
        if gas_cost > 1.0 {
            issues.push(format!("Gas cost alone is ${:.2} (likely unprofitable)", gas_cost));
            status = ValidationStatus::Fail;
        }
    }

    ProfitValidation {
        intent_id,
        protocol,
        src_chain,
        dst_chain,
        raw_profit_usd: raw_profit,
        recalculated_profit_usd: recalculated_profit,
        gas_cost_usd: total_gas_cost_usd,
        src_gas_gwei,
        dst_gas_gwei,
        issues,
        status,
    }
}

#[tokio::test]
async fn test_all_fixtures() {
    println!("\n{'='}==========================================================================");
    println!("TAIFOON FIXTURE VALIDATOR");
    println!("{'='}==========================================================================\n");

    // Fetch gas prices from Razor API
    println!("📡 Fetching gas prices from Razor API...");
    let gas_prices = match fetch_razor_gas_prices().await {
        Ok(prices) => {
            println!("✅ Fetched gas prices for {} chains", prices.len());
            prices
        }
        Err(e) => {
            eprintln!("❌ Failed to fetch gas prices: {}", e);
            eprintln!("   Make sure taifoon-solver is running on localhost:9081");
            panic!("Cannot proceed without gas prices");
        }
    };

    // Print gas prices
    println!("\nGas Prices:");
    for (chain_id, preset) in &gas_prices {
        if let Some(gwei) = preset.gas_price_gwei.or(preset.gas_cost_gwei) {
            println!("  Chain {}: {} gwei", chain_id, gwei);
        }
    }
    println!();

    let fixtures_dir = Path::new("fixtures");
    let protocols = vec![
        "across_v3",
        "allbridge",
        "hyperlane",
        "layerzero_v2",
        "lifi_v2",
        "orbiter_finance",
        "squid_router",
        "stargate_v2",
        "t3rn_lwc",
    ];

    let mut report = TestReport::new();

    for protocol in protocols {
        let fixture_path = fixtures_dir.join(format!("{}.json", protocol));

        if !fixture_path.exists() {
            println!("⚠️  Skipping {} (file not found)", protocol);
            continue;
        }

        println!("🔍 Testing {} fixtures...", protocol);

        let intents = match load_fixtures(&fixture_path) {
            Ok(intents) => intents,
            Err(e) => {
                eprintln!("❌ Failed to load {}: {}", protocol, e);
                continue;
            }
        };

        for intent in intents {
            let validation = validate_intent(&intent, &gas_prices);
            report.add(validation);
        }

        println!("   ✓ Validated {} intents", report.total_intents);
    }

    // Print comprehensive report
    report.print_summary();
    report.protocol_breakdown();
    report.print_failures();
    report.print_warnings();

    // Assert that we have reasonable pass rate
    let pass_rate = (report.passed as f64 / report.total_intents as f64) * 100.0;
    assert!(
        report.failed == 0,
        "Found {} failing intents (gas pricing bugs still present)",
        report.failed
    );

    println!("{'='}==========================================================================");
    println!("✅ ALL TESTS PASSED (Pass rate: {:.1}%)", pass_rate);
    println!("{'='}==========================================================================\n");
}
