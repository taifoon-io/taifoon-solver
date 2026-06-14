//! Integration tests for protocol adapters
//!
//! Tests the complete lifecycle for each protocol:
//! 1. Intent detection & adapter selection
//! 2. Gas estimation via Spinner API
//! 3. Fill transaction building
//! 4. Execution (simulated)
//! 5. Fund claiming (simulated)

use protocol_adapters::*;
use genome_client::Intent;

/// Create a mock Intent for testing
fn create_test_intent(protocol: &str, src_chain: u64, dst_chain: u64) -> Intent {
    Intent {
        id: format!("{}:test_order_12345", protocol),
        protocol: protocol.to_string(),
        src_chain,
        dst_chain,
        src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // USDC
        dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(), // USDC on Arb
        amount: "1000000".to_string(), // 1 USDC (6 decimals)
        depositor: "0x1234567890123456789012345678901234567890".to_string(),
        recipient: "0x1234567890123456789012345678901234567890".to_string(),
        tx_hash: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
        detected_at: 1234567890,
        ..Default::default()
    }
}

/// Create a mock V5 proof for testing
fn create_test_proof() -> V5ProofBlob {
    V5ProofBlob {
        l1_superroot: L1SuperRoot {
            hash: "0x1234567890abcdef".to_string(),
            timestamp: 1234567890,
            chains_included: vec![1, 42161],
        },
        l2_chain_header: L2ChainHeader {
            chain_id: 1,
            block_number: 1000000,
            block_hash: "0xblock123".to_string(),
            parent_hash: "0xparent123".to_string(),
            state_root: "0xstate123".to_string(),
            timestamp: 1234567890,
        },
        l3_superroot_proof: vec![],
        l4_block_proof: vec![],
        l5_chain_event: L5ChainEvent {
            tx_hash: "0xtx123".to_string(),
            tx_index: 0,
            log_index: Some(0),
            encoded_tx: "0x".to_string(),
            encoded_receipt: "0x".to_string(),
        },
        l6_finality: L6FinalityCommitment {
            finality_type: "ETH_POS_CHECKPOINT".to_string(),
            commitment_data: "{}".to_string(),
        },
    }
}

#[tokio::test]
async fn test_across_full_lifecycle() {
    println!("\n🔵 Testing Across V3 Full Lifecycle\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");
    let adapter = AcrossAdapter::new(spinner_client);
    let intent = create_test_intent("across_v3", 1, 42161);
    let proof = create_test_proof();

    // Step 1: Check if adapter can handle the intent
    println!("1️⃣  Checking if Across adapter can handle intent...");
    assert!(adapter.can_handle(&intent), "Across adapter should handle across_v3 intents");
    println!("   ✅ Across adapter can handle this intent");

    // Step 2: Build fill transaction
    println!("\n2️⃣  Building Across fillV3Relay transaction...");
    let fill_tx = adapter.build_fill_tx(&intent, &proof).await;
    assert!(fill_tx.is_ok(), "Should build fill tx successfully");
    let fill_tx = fill_tx.unwrap();
    println!("   ✅ Fill transaction built:");
    println!("      To: {}", fill_tx.to);
    println!("      Chain: {}", fill_tx.chain_id);
    println!("      Calldata length: {} bytes", fill_tx.data.len());

    // Step 3: Execute fill (simulated)
    println!("\n3️⃣  Executing fill transaction (SIMULATION)...");
    let fill_result = adapter.execute_fill(&intent, fill_tx, true).await;
    assert!(fill_result.is_ok(), "Simulated execution should succeed");
    let fill_result = fill_result.unwrap();
    assert!(fill_result.simulated, "Should be marked as simulated");
    assert!(fill_result.success, "Simulation should succeed");
    println!("   ✅ Fill executed (simulated):");
    println!("      Tx hash: {}", fill_result.tx_hash);
    println!("      Gas used: {}", fill_result.gas_used);

    // Step 4: Claim funds (simulated)
    println!("\n4️⃣  Claiming funds on source chain (SIMULATION)...");
    let claim_result = adapter.claim_funds(&intent, &fill_result).await;
    assert!(claim_result.is_ok(), "Claim should succeed");
    let claim_result = claim_result.unwrap();
    println!("   ✅ Funds claimed (simulated):");
    println!("      Tx hash: {}", claim_result.tx_hash);
    println!("      Amount: {}", claim_result.claimed_amount);
    println!("      Token: {}", claim_result.claimed_token);

    println!("\n✅ Across V3 full lifecycle test PASSED\n");
}

#[tokio::test]
async fn test_debridge_full_lifecycle() {
    println!("\n🟣 Testing deBridge DLN Full Lifecycle\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");
    let adapter = DeBridgeAdapter::new(spinner_client);
    let intent = create_test_intent("debridge_dln", 1, 42161);
    let proof = create_test_proof();

    // Step 1: Check if adapter can handle the intent
    println!("1️⃣  Checking if deBridge adapter can handle intent...");
    assert!(adapter.can_handle(&intent), "deBridge adapter should handle debridge_dln intents");
    println!("   ✅ deBridge adapter can handle this intent");

    // Step 2: Build fill transaction
    println!("\n2️⃣  Building deBridge fulfillOrder transaction...");
    let fill_tx = adapter.build_fill_tx(&intent, &proof).await;
    assert!(fill_tx.is_ok(), "Should build fill tx successfully");
    let fill_tx = fill_tx.unwrap();
    println!("   ✅ Fill transaction built:");
    println!("      To: {}", fill_tx.to);
    println!("      Chain: {}", fill_tx.chain_id);
    println!("      Calldata length: {} bytes", fill_tx.data.len());

    // Step 3: Execute fill (simulated)
    println!("\n3️⃣  Executing fill transaction (SIMULATION)...");
    let fill_result = adapter.execute_fill(&intent, fill_tx, true).await;
    assert!(fill_result.is_ok(), "Simulated execution should succeed");
    let fill_result = fill_result.unwrap();
    assert!(fill_result.simulated, "Should be marked as simulated");
    assert!(fill_result.success, "Simulation should succeed");
    println!("   ✅ Fill executed (simulated):");
    println!("      Tx hash: {}", fill_result.tx_hash);
    println!("      Gas used: {}", fill_result.gas_used);

    // Step 4: Claim funds (simulated)
    println!("\n4️⃣  Claiming funds on source chain (SIMULATION)...");
    let claim_result = adapter.claim_funds(&intent, &fill_result).await;
    assert!(claim_result.is_ok(), "Claim should succeed");
    let claim_result = claim_result.unwrap();
    println!("   ✅ Funds claimed (simulated):");
    println!("      Tx hash: {}", claim_result.tx_hash);
    println!("      Amount: {}", claim_result.claimed_amount);
    println!("      Token: {}", claim_result.claimed_token);

    println!("\n✅ deBridge DLN full lifecycle test PASSED\n");
}

#[tokio::test]
async fn test_mayan_full_lifecycle() {
    println!("\n🟡 Testing Mayan Finance Full Lifecycle\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");
    let adapter = MayanAdapter::new(spinner_client);
    let intent = create_test_intent("mayan_finance", 1, 42161);
    let proof = create_test_proof();

    // Step 1: Check if adapter can handle the intent
    println!("1️⃣  Checking if Mayan adapter can handle intent...");
    assert!(adapter.can_handle(&intent), "Mayan adapter should handle mayan_finance intents");
    println!("   ✅ Mayan adapter can handle this intent");

    // Step 2: Build fill transaction
    println!("\n2️⃣  Building Mayan fulfill transaction...");
    let fill_tx = adapter.build_fill_tx(&intent, &proof).await;
    assert!(fill_tx.is_ok(), "Should build fill tx successfully");
    let fill_tx = fill_tx.unwrap();
    println!("   ✅ Fill transaction built:");
    println!("      To: {}", fill_tx.to);
    println!("      Chain: {}", fill_tx.chain_id);
    println!("      Calldata length: {} bytes", fill_tx.data.len());

    // Step 3: Execute fill (simulated)
    println!("\n3️⃣  Executing fill transaction (SIMULATION)...");
    let fill_result = adapter.execute_fill(&intent, fill_tx, true).await;
    assert!(fill_result.is_ok(), "Simulated execution should succeed");
    let fill_result = fill_result.unwrap();
    assert!(fill_result.simulated, "Should be marked as simulated");
    assert!(fill_result.success, "Simulation should succeed");
    println!("   ✅ Fill executed (simulated):");
    println!("      Tx hash: {}", fill_result.tx_hash);
    println!("      Gas used: {}", fill_result.gas_used);

    // Step 4: Claim funds (simulated)
    println!("\n4️⃣  Claiming funds (automatic settlement)...");
    let claim_result = adapter.claim_funds(&intent, &fill_result).await;
    assert!(claim_result.is_ok(), "Claim should succeed");
    let claim_result = claim_result.unwrap();
    println!("   ✅ Funds claimed (automatic settlement):");
    println!("      Tx hash: {}", claim_result.tx_hash);
    println!("      Amount: {}", claim_result.claimed_amount);
    println!("      Token: {}", claim_result.claimed_token);

    println!("\n✅ Mayan Finance full lifecycle test PASSED\n");
}

#[tokio::test]
async fn test_adapter_factory() {
    println!("\n🏭 Testing AdapterFactory\n");

    let factory = AdapterFactory::new("https://api.taifoon.dev");

    // Test Across
    println!("1️⃣  Testing Across adapter creation...");
    let across_intent = create_test_intent("across_v3", 1, 42161);
    let across_adapter = factory.get_adapter(&across_intent);
    assert!(across_adapter.is_ok(), "Should create Across adapter");
    assert_eq!(across_adapter.unwrap().protocol_name(), "across_v3");
    println!("   ✅ Across adapter created");

    // Test deBridge
    println!("\n2️⃣  Testing deBridge adapter creation...");
    let debridge_intent = create_test_intent("debridge_dln", 1, 42161);
    let debridge_adapter = factory.get_adapter(&debridge_intent);
    assert!(debridge_adapter.is_ok(), "Should create deBridge adapter");
    assert_eq!(debridge_adapter.unwrap().protocol_name(), "debridge_dln");
    println!("   ✅ deBridge adapter created");

    // Test Mayan
    println!("\n3️⃣  Testing Mayan adapter creation...");
    let mayan_intent = create_test_intent("mayan_finance", 1, 42161);
    let mayan_adapter = factory.get_adapter(&mayan_intent);
    assert!(mayan_adapter.is_ok(), "Should create Mayan adapter");
    assert_eq!(mayan_adapter.unwrap().protocol_name(), "mayan_finance");
    println!("   ✅ Mayan adapter created");

    // Test unsupported protocol
    println!("\n4️⃣  Testing unsupported protocol...");
    let unknown_intent = create_test_intent("unknown_protocol", 1, 42161);
    let unknown_adapter = factory.get_adapter(&unknown_intent);
    assert!(unknown_adapter.is_err(), "Should fail for unsupported protocol");
    println!("   ✅ Correctly rejected unsupported protocol");

    // Test supported protocols list
    println!("\n5️⃣  Testing supported protocols list...");
    let supported = factory.supported_protocols();
    assert!(supported.contains(&"across"));
    assert!(supported.contains(&"debridge"));
    assert!(supported.contains(&"mayan"));
    println!("   ✅ Supported protocols: {:?}", supported);

    println!("\n✅ AdapterFactory test PASSED\n");
}

#[tokio::test]
async fn test_protocol_routing() {
    println!("\n🔀 Testing Protocol Routing\n");

    let factory = AdapterFactory::new("https://api.taifoon.dev");

    // Test various protocol name variations
    let test_cases = vec![
        ("across", "across_v3"),
        ("across_v3", "across_v3"),
        ("ACROSS", "across_v3"),
        ("debridge", "debridge_dln"),
        ("debridge_dln", "debridge_dln"),
        ("DeBridge", "debridge_dln"),
        ("mayan", "mayan_finance"),
        ("mayan_finance", "mayan_finance"),
        ("Mayan", "mayan_finance"),
    ];

    for (input_protocol, expected_name) in test_cases {
        let intent = create_test_intent(input_protocol, 1, 42161);
        let adapter = factory.get_adapter(&intent);
        assert!(adapter.is_ok(), "Should handle protocol: {}", input_protocol);
        assert_eq!(
            adapter.unwrap().protocol_name(),
            expected_name,
            "Protocol {} should map to {}",
            input_protocol,
            expected_name
        );
        println!("   ✅ {} → {}", input_protocol, expected_name);
    }

    println!("\n✅ Protocol routing test PASSED\n");
}

#[tokio::test]
async fn test_multi_chain_support() {
    println!("\n🌐 Testing Multi-Chain Support\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");

    // Test Across on multiple chains
    println!("1️⃣  Testing Across multi-chain support...");
    let across_adapter = AcrossAdapter::new(spinner_client.clone());
    let chains = vec![
        (1, "Ethereum"),
        (10, "Optimism"),
        (42161, "Arbitrum"),
        (8453, "Base"),
        (137, "Polygon"),
    ];

    for (chain_id, chain_name) in &chains {
        let intent = create_test_intent("across_v3", 1, *chain_id);
        let fill_tx = across_adapter.build_fill_tx(&intent, &create_test_proof()).await;
        assert!(fill_tx.is_ok(), "Across should support {}", chain_name);
        println!("   ✅ Across supports {} (chain {})", chain_name, chain_id);
    }

    // Test deBridge on multiple chains
    println!("\n2️⃣  Testing deBridge multi-chain support...");
    let debridge_adapter = DeBridgeAdapter::new(spinner_client.clone());
    let dln_chains = vec![
        (1, "Ethereum"),
        (10, "Optimism"),
        (42161, "Arbitrum"),
        (8453, "Base"),
        (56, "BSC"),
        (43114, "Avalanche"),
        (59144, "Linea"),
    ];

    for (chain_id, chain_name) in &dln_chains {
        let intent = create_test_intent("debridge_dln", 1, *chain_id);
        let fill_tx = debridge_adapter.build_fill_tx(&intent, &create_test_proof()).await;
        assert!(fill_tx.is_ok(), "deBridge should support {}", chain_name);
        println!("   ✅ deBridge supports {} (chain {})", chain_name, chain_id);
    }

    // Test Mayan on multiple chains
    println!("\n3️⃣  Testing Mayan multi-chain support...");
    let mayan_adapter = MayanAdapter::new(spinner_client);
    let mayan_chains = vec![
        (1, "Ethereum"),
        (10, "Optimism"),
        (42161, "Arbitrum"),
        (8453, "Base"),
    ];

    for (chain_id, chain_name) in &mayan_chains {
        let intent = create_test_intent("mayan_finance", 1, *chain_id);
        let fill_tx = mayan_adapter.build_fill_tx(&intent, &create_test_proof()).await;
        assert!(fill_tx.is_ok(), "Mayan should support {}", chain_name);
        println!("   ✅ Mayan supports {} (chain {})", chain_name, chain_id);
    }

    println!("\n✅ Multi-chain support test PASSED\n");
}

// ── New adapter tests (Stargate / Relay / CCTP / best-quote selection) ────────

fn create_test_proof_local() -> V5ProofBlob {
    V5ProofBlob {
        l1_superroot: L1SuperRoot {
            hash: "0x1234567890abcdef".to_string(),
            timestamp: 1234567890,
            chains_included: vec![1, 42161],
        },
        l2_chain_header: L2ChainHeader {
            chain_id: 1, block_number: 1_000_000,
            block_hash: "0xblock".to_string(), parent_hash: "0xparent".to_string(),
            state_root: "0xstate".to_string(), timestamp: 1234567890,
        },
        l3_superroot_proof: vec![], l4_block_proof: vec![],
        l5_chain_event: L5ChainEvent {
            tx_hash: "0xtx".to_string(), tx_index: 0, log_index: Some(0),
            encoded_tx: "0x".to_string(), encoded_receipt: "0x".to_string(),
        },
        l6_finality: L6FinalityCommitment {
            finality_type: "ETH_POS_CHECKPOINT".to_string(), commitment_data: "{}".to_string(),
        },
    }
}

#[tokio::test]
async fn test_stargate_adapter_full_lifecycle() {
    println!("\n🔷 Testing Stargate V2 Full Lifecycle\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");
    let adapter = StargateAdapter::new(spinner_client);
    let intent = create_test_intent("stargate_v2", 1, 42161);
    let proof = create_test_proof_local();

    assert!(adapter.can_handle(&intent));
    assert_eq!(adapter.protocol_name(), "stargate_v2");
    println!("   ✅ Stargate adapter handles stargate_v2 intent");

    let fill_tx = adapter.build_fill_tx(&intent, &proof).await.unwrap();
    assert_eq!(fill_tx.chain_id, 1); // src chain (Stargate sends from source)
    assert!(fill_tx.data.starts_with("0x"));
    assert!(fill_tx.data.len() > 10);
    println!("   ✅ Fill tx built: to={}, chain={}", fill_tx.to, fill_tx.chain_id);

    let result = adapter.execute_fill(&intent, fill_tx, true).await.unwrap();
    assert!(result.simulated);
    assert!(result.success);
    println!("   ✅ Simulated fill succeeded: {}", result.tx_hash);

    let fill_result = FillResult { tx_hash: result.tx_hash, gas_used: result.gas_used, block_number: 0, success: true, simulated: true };
    let claim = adapter.claim_funds(&intent, &fill_result).await.unwrap();
    assert!(!claim.claimed_amount.is_empty());
    println!("   ✅ Claim (auto-delivery): amount={}", claim.claimed_amount);

    println!("\n✅ Stargate V2 lifecycle PASSED\n");
}

#[tokio::test]
async fn test_relay_adapter_full_lifecycle() {
    println!("\n🔶 Testing Relay Protocol Full Lifecycle\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");
    let adapter = RelayAdapter::new(spinner_client);
    let intent = create_test_intent("relay", 1, 42161);
    let proof = create_test_proof_local();

    assert!(adapter.can_handle(&intent));
    assert_eq!(adapter.protocol_name(), "relay");
    println!("   ✅ Relay adapter handles relay intent");

    let fill_tx = adapter.build_fill_tx(&intent, &proof).await.unwrap();
    assert_eq!(fill_tx.chain_id, 42161); // dst chain (Relay fills on destination)
    assert!(fill_tx.data.starts_with("0x"));
    println!("   ✅ Fill tx built: to={}, chain={}", fill_tx.to, fill_tx.chain_id);

    let result = adapter.execute_fill(&intent, fill_tx, true).await.unwrap();
    assert!(result.simulated);
    assert!(result.success);
    println!("   ✅ Simulated fill succeeded: {}", result.tx_hash);

    let fill_result = FillResult { tx_hash: result.tx_hash, gas_used: result.gas_used, block_number: 0, success: true, simulated: true };
    let claim = adapter.claim_funds(&intent, &fill_result).await.unwrap();
    assert_eq!(claim.claimed_amount, intent.amount);
    println!("   ✅ Claim (auto-settle): amount={}", claim.claimed_amount);

    println!("\n✅ Relay Protocol lifecycle PASSED\n");
}

#[tokio::test]
async fn test_cctp_adapter_full_lifecycle() {
    println!("\n🟢 Testing CCTP Full Lifecycle\n");

    let spinner_client = SpinnerClient::new("https://api.taifoon.dev");
    let adapter = CctpAdapter::new(spinner_client);
    let intent = create_test_intent("cctp", 1, 42161);
    let proof = create_test_proof_local();

    assert!(adapter.can_handle(&intent));
    assert_eq!(adapter.protocol_name(), "cctp");
    println!("   ✅ CCTP adapter handles cctp intent");

    let fill_tx = adapter.build_fill_tx(&intent, &proof).await.unwrap();
    assert_eq!(fill_tx.chain_id, 42161); // dst chain (receiveMessage on destination)
    assert!(fill_tx.data.starts_with("0x"));
    assert_eq!(fill_tx.value.as_deref(), Some("0x0"));
    println!("   ✅ Fill tx built: to={}, chain={}", fill_tx.to, fill_tx.chain_id);

    let result = adapter.execute_fill(&intent, fill_tx, true).await.unwrap();
    assert!(result.simulated);
    assert!(result.success);
    println!("   ✅ Simulated fill succeeded: {}", result.tx_hash);

    let fill_result = FillResult { tx_hash: result.tx_hash, gas_used: result.gas_used, block_number: 0, success: true, simulated: true };
    let claim = adapter.claim_funds(&intent, &fill_result).await.unwrap();
    assert_eq!(claim.claimed_amount, intent.amount);
    println!("   ✅ Claim (mint): amount={}", claim.claimed_amount);

    println!("\n✅ CCTP lifecycle PASSED\n");
}

#[tokio::test]
async fn test_factory_routes_new_adapters() {
    println!("\n🏭 Testing Factory Routes New Adapters\n");

    let factory = AdapterFactory::new("https://api.taifoon.dev");

    let cases = vec![
        ("stargate_v2", "stargate_v2"),
        ("stargate",    "stargate_v2"),
        ("STARGATE",    "stargate_v2"),
        ("relay",       "relay"),
        ("Relay",       "relay"),
        ("cctp",        "cctp"),
        ("CCTP",        "cctp"),
        ("circle_bridge", "cctp"),
    ];

    for (input, expected_name) in &cases {
        let intent = create_test_intent(input, 1, 42161);
        let adapter = factory.get_adapter(&intent).expect(&format!("should route {}", input));
        assert_eq!(adapter.protocol_name(), *expected_name,
            "protocol '{}' should map to '{}'", input, expected_name);
        println!("   ✅ {} → {}", input, adapter.protocol_name());
    }

    let supported = factory.supported_protocols();
    assert!(supported.contains(&"stargate_v2"));
    assert!(supported.contains(&"relay"));
    assert!(supported.contains(&"cctp"));
    println!("   ✅ All new protocols in supported list");

    println!("\n✅ Factory routing for new adapters PASSED\n");
}

#[tokio::test]
async fn test_best_quote_selection_includes_new_adapters() {
    println!("\n🏆 Testing Best-Quote Selection Includes New Adapters\n");

    // Use get_all_adapters() to verify Stargate/Relay/CCTP are considered in the pool.
    let factory = AdapterFactory::new("https://api.taifoon.dev");
    let proof = create_test_proof_local();

    // Scenario A: a "stargate_v2" intent — only StargateAdapter handles it
    let stargate_intent = create_test_intent("stargate_v2", 1, 42161);
    let sg_adapters = factory.get_all_adapters(&stargate_intent);
    assert!(!sg_adapters.is_empty(), "at least one adapter should handle stargate_v2");
    let sg_names: Vec<_> = sg_adapters.iter().map(|a| a.protocol_name()).collect();
    assert!(sg_names.contains(&"stargate_v2"),
        "StargateAdapter must be in the quote pool for stargate_v2 intent; got: {:?}", sg_names);
    println!("   ✅ Stargate-intent quote pool: {:?}", sg_names);

    // Scenario B: a "relay" intent — only RelayAdapter handles it
    let relay_intent = create_test_intent("relay", 1, 42161);
    let rl_adapters = factory.get_all_adapters(&relay_intent);
    let rl_names: Vec<_> = rl_adapters.iter().map(|a| a.protocol_name()).collect();
    assert!(rl_names.contains(&"relay"),
        "RelayAdapter must be in the quote pool; got: {:?}", rl_names);
    println!("   ✅ Relay-intent quote pool: {:?}", rl_names);

    // Scenario C: a "cctp" intent — only CctpAdapter handles it
    let cctp_intent = create_test_intent("cctp", 1, 42161);
    let cc_adapters = factory.get_all_adapters(&cctp_intent);
    let cc_names: Vec<_> = cc_adapters.iter().map(|a| a.protocol_name()).collect();
    assert!(cc_names.contains(&"cctp"),
        "CctpAdapter must be in the quote pool; got: {:?}", cc_names);
    println!("   ✅ CCTP-intent quote pool: {:?}", cc_names);

    // Scenario D: simulate best-quote selection — Stargate wins when its fill_tx is
    // the only successfully-built tx for a stargate intent.
    let sg_winner: Option<String> = {
        let adapters = factory.get_all_adapters(&stargate_intent);
        let mut best: Option<String> = None;
        for adapter in &adapters {
            if let Ok(_tx) = adapter.build_fill_tx(&stargate_intent, &proof).await {
                best = Some(adapter.protocol_name().to_string());
                break;
            }
        }
        best
    };
    assert!(sg_winner.is_some(), "At least one adapter must produce a valid quote");
    assert_eq!(sg_winner.as_deref(), Some("stargate_v2"),
        "For a stargate intent, StargateAdapter should win best-quote selection");
    println!("   ✅ Best-quote winner for stargate_v2 intent: {}", sg_winner.unwrap());

    // Scenario E: CCTP wins for a cctp-tagged intent (first valid fill_tx)
    let cctp_winner: Option<String> = {
        let adapters = factory.get_all_adapters(&cctp_intent);
        let mut best: Option<String> = None;
        for adapter in &adapters {
            if let Ok(_tx) = adapter.build_fill_tx(&cctp_intent, &proof).await {
                best = Some(adapter.protocol_name().to_string());
                break;
            }
        }
        best
    };
    assert_eq!(cctp_winner.as_deref(), Some("cctp"),
        "CCTP adapter should win best-quote for a cctp intent");
    println!("   ✅ Best-quote winner for cctp intent: {}", cctp_winner.unwrap());

    println!("\n✅ Best-quote selection test PASSED — all new adapters included in quote pool\n");
}
