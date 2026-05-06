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
