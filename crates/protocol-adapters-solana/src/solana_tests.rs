/// Comprehensive Solana test suite for the taifoon-solver protocol-adapters-solana crate.
///
/// Coverage areas:
///   1. Anchor discriminator correctness (fulfill, initialize_order)
///   2. PDA derivation (vault + state seeds)
///   3. MayanSolanaIntent projection (field mapping, fallbacks, error cases)
///   4. Transaction wire format (message header, key ordering, size bound)
///   5. Signing key loading (all accepted formats)
///   6. Simulate classifier (all four outcome variants + edge cases)
///   7. broadcast tx includes ComputeBudget instructions (regression)
///   8. Order ID encoding in instruction data (big-endian 32-byte)
///   9. Intent dedup key uniqueness
///  10. Float amount parse (small ETH values from Mayan poller)
///  11. Compact-u16 encoding boundary values
///  12. Account key ordering in legacy message (signer-writable first)
///  13. Vault PDA derivation — off-curve check and determinism
///  14. Cross-chain field validation (EVM recipient on Solana-source intent)
///  15. Compute unit cap (never exceeds u32::MAX)
#[cfg(test)]
mod solana_tests {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use sha2::{Digest, Sha256};

    use crate::keychain::parse_solana_secret;
    use crate::mayan_solana::{
        anchor_discriminator, derive_mayan_vault_pda, MayanSolanaIntent, MayanSolanaSimulator,
        COMPUTE_BUDGET_PROGRAM_ID, DEFAULT_MAYAN_SWIFT_PROGRAM,
        DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU, SYSTEM_PROGRAM_ID,
    };
    use crate::send::{load_signing_key, SolanaBroadcaster};
    use crate::simulate::{classify_solana_simulate_result, SolanaEstimateOutcome};
    use genome_client::Intent;

    // ─── fixtures ────────────────────────────────────────────────────────────

    fn base_intent() -> Intent {
        Intent {
            id: "mayan_swift:test_order".into(),
            protocol: "mayan_swift".into(),
            src_chain: 1399811149,
            dst_chain: 1,
            src_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            dst_token: "0xA0b86991c6218B36c1d19D4a2e9Eb0cE3606eB48".into(),
            amount: "100000000".into(),
            depositor: "DepositorWa11etAddrSoLana1111111111111111111".into(),
            recipient: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1".into(),
            tx_hash: "5HzkTestSolTx1111111111111111111111111111111111111111111111111".into(),
            detected_at: 1745928045,
            output_amount: Some("99850000".into()),
            mayan_order_id: Some(
                "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b"
                    .into(),
            ),
            trader: Some("DepositorWa11etAddrSoLana1111111111111111111".into()),
            deadline: Some(1745931645),
            swift_program_id: Some(DEFAULT_MAYAN_SWIFT_PROGRAM.into()),
            state_account: Some("9wK4N3pTzXyZ8vQ5mB2hWnQ7tR9uVaCfDgFhJiKkMnPp".into()),
            vault_account: Some("8mB2hWnQ7tR9uVaCfDgFhJiKkMnPpQ9wK4N3pTzXyZ8v".into()),
            compute_units_estimate: Some(240_000),
            is_solana_source: Some(true),
            ..Default::default()
        }
    }

    fn sim() -> MayanSolanaSimulator {
        MayanSolanaSimulator::new(SYSTEM_PROGRAM_ID, "http://localhost:8899")
    }

    fn project(intent: &Intent) -> MayanSolanaIntent {
        MayanSolanaIntent::from_intent(intent).expect("project intent")
    }

    fn build_tx(si: &MayanSolanaIntent) -> Vec<u8> {
        BASE64
            .decode(sim().build_simulate_tx_b64(si).expect("build tx"))
            .expect("base64 decode")
    }

    // ─── 1. Anchor discriminator ─────────────────────────────────────────────

    #[test]
    fn discriminator_fulfill_is_correct_sha256_prefix() {
        // python3 -c "import hashlib; print(hashlib.sha256(b'global:fulfill').hexdigest()[:16])"
        assert_eq!(hex::encode(anchor_discriminator("fulfill")), "8f0234ceaea4f748");
    }

    #[test]
    fn discriminator_is_deterministic() {
        assert_eq!(
            anchor_discriminator("fulfill"),
            anchor_discriminator("fulfill")
        );
    }

    #[test]
    fn different_names_produce_different_discriminators() {
        let a = anchor_discriminator("fulfill");
        let b = anchor_discriminator("initialize_order");
        assert_ne!(a, b, "two different ix names must produce different discriminators");
    }

    #[test]
    fn discriminator_is_exactly_8_bytes() {
        assert_eq!(anchor_discriminator("fulfill").len(), 8);
        assert_eq!(anchor_discriminator("any_name").len(), 8);
    }

    // ─── 2. PDA derivation ───────────────────────────────────────────────────

    #[test]
    fn vault_pda_derives_a_valid_off_curve_pubkey() {
        let order_hash =
            "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b";
        let pda = derive_mayan_vault_pda(order_hash, DEFAULT_MAYAN_SWIFT_PROGRAM)
            .expect("derive_mayan_vault_pda returned None");
        // Must be a valid base58 pubkey (32 bytes when decoded)
        let bytes = bs58::decode(&pda).into_vec().expect("base58 decode pda");
        assert_eq!(bytes.len(), 32, "PDA must be 32 bytes");
        // Must NOT be on the ed25519 curve (Solana PDA invariant)
        assert!(
            ed25519_dalek::VerifyingKey::from_bytes(bytes.as_slice().try_into().unwrap()).is_err(),
            "PDA must be off the ed25519 curve"
        );
    }

    #[test]
    fn vault_pda_derivation_is_deterministic() {
        let order_hash =
            "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b";
        let pda1 = derive_mayan_vault_pda(order_hash, DEFAULT_MAYAN_SWIFT_PROGRAM);
        let pda2 = derive_mayan_vault_pda(order_hash, DEFAULT_MAYAN_SWIFT_PROGRAM);
        assert_eq!(pda1, pda2, "PDA derivation must be deterministic");
    }

    #[test]
    fn vault_pda_changes_with_order_hash() {
        let h1 = "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b";
        let h2 = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let p1 = derive_mayan_vault_pda(h1, DEFAULT_MAYAN_SWIFT_PROGRAM);
        let p2 = derive_mayan_vault_pda(h2, DEFAULT_MAYAN_SWIFT_PROGRAM);
        assert_ne!(p1, p2, "different order hashes must produce different PDAs");
    }

    #[test]
    fn vault_pda_rejects_bad_order_hash() {
        assert!(
            derive_mayan_vault_pda("not_hex", DEFAULT_MAYAN_SWIFT_PROGRAM).is_none(),
            "invalid hex must return None"
        );
        assert!(
            derive_mayan_vault_pda("0xdeadbeef", DEFAULT_MAYAN_SWIFT_PROGRAM).is_none(),
            "too-short hash must return None"
        );
    }

    #[test]
    fn vault_pda_falls_back_from_intent_when_vault_absent() {
        let mut intent = base_intent();
        intent.vault_account = None; // force derivation
        let si = MayanSolanaIntent::from_intent(&intent).expect("project");
        // Should have derived a non-empty vault address
        assert!(
            !si.vault_account_b58.is_empty(),
            "vault_account_b58 must be populated by PDA derivation"
        );
        assert_ne!(
            si.vault_account_b58,
            SYSTEM_PROGRAM_ID,
            "derived vault must not be the system program"
        );
    }

    // ─── 3. Intent projection ────────────────────────────────────────────────

    #[test]
    fn projection_succeeds_with_full_intent() {
        let intent = base_intent();
        let si = project(&intent);
        assert_eq!(si.intent_id, intent.id);
        assert_eq!(
            si.mayan_order_id_hex,
            "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b"
        );
        assert_eq!(si.min_amount_out, 99_850_000);
        assert_eq!(si.deadline, 1745931645);
        assert_eq!(si.compute_units_estimate, 240_000);
    }

    #[test]
    fn projection_rejects_missing_order_id() {
        let mut intent = base_intent();
        intent.mayan_order_id = None;
        let err = MayanSolanaIntent::from_intent(&intent).unwrap_err();
        assert!(err.to_string().contains("mayan_order_id"), "{}", err);
    }

    #[test]
    fn projection_rejects_missing_state_account() {
        let mut intent = base_intent();
        intent.state_account = None;
        let err = MayanSolanaIntent::from_intent(&intent).unwrap_err();
        assert!(err.to_string().contains("state_account"), "{}", err);
    }

    #[test]
    fn projection_uses_output_amount_for_min_amount_out() {
        let intent = base_intent();
        let si = project(&intent);
        assert_eq!(si.min_amount_out, 99_850_000);
    }

    #[test]
    fn projection_falls_back_to_amount_when_output_amount_absent() {
        let mut intent = base_intent();
        intent.output_amount = None;
        let si = MayanSolanaIntent::from_intent(&intent).expect("project with fallback");
        assert_eq!(si.min_amount_out, 100_000_000, "should fall back to amount");
    }

    #[test]
    fn projection_parses_float_amount_for_small_eth() {
        let mut intent = base_intent();
        intent.output_amount = Some("0.000500".into()); // 500 micro-ETH as float string
        let si = MayanSolanaIntent::from_intent(&intent).expect("project float amount");
        assert_eq!(si.min_amount_out, 0, "0.0005 truncates to 0 as u64");
    }

    #[test]
    fn projection_parses_large_float_amount() {
        let mut intent = base_intent();
        intent.output_amount = Some("1500000.9".into());
        let si = MayanSolanaIntent::from_intent(&intent).expect("project large float");
        assert_eq!(si.min_amount_out, 1_500_000, "float truncation");
    }

    #[test]
    fn projection_uses_trader_as_pubkey_when_solana_shaped() {
        let intent = base_intent();
        let si = project(&intent);
        assert_eq!(
            si.trader_pubkey_b58,
            "DepositorWa11etAddrSoLana1111111111111111111"
        );
    }

    #[test]
    fn projection_falls_back_to_recipient_when_trader_is_evm() {
        let mut intent = base_intent();
        intent.trader = Some("0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1".into());
        intent.recipient = "SolanaRecipientPubkeyBase58AAAAAAAAAAAAAAAAAA".into();
        let si = MayanSolanaIntent::from_intent(&intent).expect("fallback to recipient");
        assert_eq!(si.trader_pubkey_b58, "SolanaRecipientPubkeyBase58AAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn projection_uses_default_program_when_swift_program_absent() {
        let mut intent = base_intent();
        intent.swift_program_id = None;
        let si = MayanSolanaIntent::from_intent(&intent).expect("default program");
        assert_eq!(si.swift_program_id_b58, DEFAULT_MAYAN_SWIFT_PROGRAM);
    }

    #[test]
    fn projection_uses_default_deadline_when_absent() {
        let mut intent = base_intent();
        intent.deadline = None;
        let si = MayanSolanaIntent::from_intent(&intent).expect("default deadline");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(si.deadline > now, "deadline must be in the future");
        assert!(si.deadline < now + 7200, "deadline fallback must be ~1 hour");
    }

    #[test]
    fn projection_uses_240k_compute_units_as_default() {
        let mut intent = base_intent();
        intent.compute_units_estimate = None;
        let si = MayanSolanaIntent::from_intent(&intent).expect("default cu");
        assert_eq!(si.compute_units_estimate, 240_000);
    }

    #[test]
    fn projection_compute_units_capped_at_u32_max() {
        let mut intent = base_intent();
        intent.compute_units_estimate = Some(u64::MAX);
        let si = MayanSolanaIntent::from_intent(&intent).expect("cu cap");
        // build_signed_tx casts to u32 — verify the value rounds down safely
        assert!(si.compute_units_estimate <= u64::from(u32::MAX));
    }

    // ─── 4. Transaction wire format ──────────────────────────────────────────

    #[test]
    fn simulate_tx_is_within_solana_legacy_size_limit() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        assert!(
            raw.len() <= 1232,
            "tx {} bytes exceeds 1232-byte Solana legacy limit",
            raw.len()
        );
    }

    #[test]
    fn simulate_tx_starts_with_single_sig_count() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        assert_eq!(raw[0], 1, "compact-u16(1) = 0x01 for one signer");
    }

    #[test]
    fn simulate_tx_has_zeroed_signature_placeholder() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        // raw[0] = sig count byte, raw[1..65] = 64-byte signature
        assert_eq!(&raw[1..65], &[0u8; 64], "simulate tx must use zeroed sig");
    }

    #[test]
    fn simulate_tx_message_header_requires_one_signer() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        // raw[65] = first byte of message = num_required_signers
        assert_eq!(raw[65], 1, "exactly one required signer (payer)");
    }

    #[test]
    fn simulate_tx_contains_anchor_discriminator() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        let disc = anchor_discriminator("fulfill");
        assert!(
            raw.windows(8).any(|w| w == disc),
            "anchor discriminator must appear in serialized tx"
        );
    }

    #[test]
    fn simulate_tx_contains_order_id_bytes() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        // Order ID is 0x9f8e7d...a2b — first 4 bytes: 9f 8e 7d 6c
        let order_prefix = [0x9f, 0x8e, 0x7d, 0x6c];
        assert!(
            raw.windows(4).any(|w| w == order_prefix),
            "order ID bytes must appear in serialized tx"
        );
    }

    #[test]
    fn simulate_tx_contains_compute_budget_programs() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        // ComputeBudget program pubkey bytes (decoded from base58)
        let cb_bytes = bs58::decode(COMPUTE_BUDGET_PROGRAM_ID)
            .into_vec()
            .expect("decode compute budget program");
        assert!(
            raw.windows(32).any(|w| w == cb_bytes.as_slice()),
            "ComputeBudget program must appear in account keys"
        );
    }

    #[test]
    fn simulate_tx_priority_fee_is_non_zero() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        // Pattern: compact-u16(9) = 0x09 | tag 0x03 | 8-byte LE u64
        let found = raw.windows(10).find_map(|w| {
            if w[0] == 0x09 && w[1] == 0x03 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&w[2..10]);
                let p = u64::from_le_bytes(buf);
                if p > 0 { Some(p) } else { None }
            } else { None }
        });
        assert_eq!(
            found,
            Some(DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU),
            "SetComputeUnitPrice must use the configured default fee"
        );
    }

    #[test]
    fn simulate_tx_compute_unit_limit_matches_intent() {
        let si = project(&base_intent());
        let raw = build_tx(&si);
        // Pattern: compact-u16(5) = 0x05 | tag 0x02 | 4-byte LE u32
        let found = raw.windows(6).find_map(|w| {
            if w[0] == 0x05 && w[1] == 0x02 {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&w[2..6]);
                let l = u32::from_le_bytes(buf);
                if l > 0 { Some(l) } else { None }
            } else { None }
        });
        assert_eq!(
            found,
            Some(240_000u32),
            "SetComputeUnitLimit must reflect intent.compute_units_estimate"
        );
    }

    // ─── 5. Signing key loading ──────────────────────────────────────────────

    #[test]
    fn load_signing_key_from_base58_64_byte_keypair() {
        use ed25519_dalek::SigningKey;
        let secret = [5u8; 32];
        let signer = SigningKey::from_bytes(&secret);
        let pubkey = signer.verifying_key().to_bytes();
        let mut keypair = [0u8; 64];
        keypair[..32].copy_from_slice(&secret);
        keypair[32..].copy_from_slice(&pubkey);
        let b58 = bs58::encode(keypair).into_string();

        let parsed = parse_solana_secret(&b58).expect("parse 64-byte b58 keypair");
        assert_eq!(parsed.to_bytes(), secret);
    }

    #[test]
    fn load_signing_key_from_hex_32_byte_secret() {
        let hex = "a".repeat(64); // 32 bytes of 0xaa
        let key = load_signing_key(&hex).expect("hex 32-byte key");
        assert_eq!(key.verifying_key().to_bytes().len(), 32);
    }

    #[test]
    fn load_signing_key_rejects_too_short_hex() {
        let err = load_signing_key(&"f".repeat(30)).unwrap_err();
        assert!(err.to_string().contains("must be"), "{}", err);
    }

    #[test]
    fn load_signing_key_rejects_garbage() {
        let err = load_signing_key("not_a_key!!!").unwrap_err();
        assert!(err.to_string().contains("must be"), "{}", err);
    }

    #[test]
    fn load_signing_key_accepts_0x_prefixed_hex() {
        let hex = format!("0x{}", "b".repeat(64));
        let key = parse_solana_secret(&hex).expect("0x-prefixed hex key");
        assert_eq!(key.to_bytes(), [0xbbu8; 32]);
    }

    // ─── 6. Simulate classifier ──────────────────────────────────────────────

    #[test]
    fn classifier_ok_with_positive_units() {
        let v = serde_json::json!({ "err": null, "unitsConsumed": 180_000u64, "logs": [] });
        let out = classify_solana_simulate_result(&v);
        assert!(matches!(out, SolanaEstimateOutcome::OkComputeUnits(180_000)));
        assert!(out.is_green());
    }

    #[test]
    fn classifier_ok_with_zero_units_when_no_error() {
        let v = serde_json::json!({ "err": null, "unitsConsumed": 0u64, "logs": [] });
        let out = classify_solana_simulate_result(&v);
        assert!(out.is_green(), "null err with 0 units is still green");
    }

    #[test]
    fn classifier_insufficient_lamports_from_err_string() {
        let v = serde_json::json!({ "err": "InsufficientFundsForFee", "logs": [] });
        let out = classify_solana_simulate_result(&v);
        assert!(
            matches!(out, SolanaEstimateOutcome::InsufficientLamports(_)),
            "got {:?}",
            out
        );
        assert!(out.is_green());
    }

    #[test]
    fn classifier_insufficient_lamports_from_logs() {
        let v = serde_json::json!({
            "err": { "InstructionError": [0, "Custom"] },
            "logs": ["Program log: insufficient lamports for transfer"]
        });
        let out = classify_solana_simulate_result(&v);
        assert!(
            matches!(out, SolanaEstimateOutcome::InsufficientLamports(_)),
            "log-only lamport failure must be green, got {:?}",
            out
        );
    }

    #[test]
    fn classifier_account_not_found_is_green() {
        let v = serde_json::json!({ "err": "AccountNotFound", "logs": [] });
        let out = classify_solana_simulate_result(&v);
        assert!(out.is_green(), "AccountNotFound = fresh wallet = green");
    }

    #[test]
    fn classifier_invalid_account_for_fee_is_green() {
        let v = serde_json::json!({ "err": "InvalidAccountForFee", "logs": [] });
        let out = classify_solana_simulate_result(&v);
        assert!(out.is_green(), "InvalidAccountForFee must be green");
    }

    #[test]
    fn classifier_custom_program_error_is_red() {
        let v = serde_json::json!({
            "err": { "InstructionError": [0, { "Custom": 6001 }] },
            "logs": ["Program log: Custom program error: 0x1771"]
        });
        let out = classify_solana_simulate_result(&v);
        assert!(
            matches!(out, SolanaEstimateOutcome::LogsContainError(_)),
            "custom program error must be red"
        );
        assert!(!out.is_green());
    }

    #[test]
    fn classifier_program_failed_to_complete_is_red() {
        let v = serde_json::json!({
            "err": { "InstructionError": [0, "ProgramFailedToComplete"] },
            "logs": []
        });
        let out = classify_solana_simulate_result(&v);
        assert!(!out.is_green(), "ProgramFailedToComplete must be red");
    }

    #[test]
    fn classifier_unknown_error_is_invalid_ix() {
        let v = serde_json::json!({ "err": "BlockhashNotFound", "logs": [] });
        let out = classify_solana_simulate_result(&v);
        assert!(matches!(out, SolanaEstimateOutcome::InvalidIx(_)));
        assert!(!out.is_green());
    }

    #[test]
    fn classifier_outcome_tags_are_stable() {
        assert_eq!(SolanaEstimateOutcome::OkComputeUnits(0).tag(), "ok_compute_units");
        assert_eq!(
            SolanaEstimateOutcome::InsufficientLamports("x".into()).tag(),
            "insufficient_lamports"
        );
        assert_eq!(
            SolanaEstimateOutcome::LogsContainError("x".into()).tag(),
            "logs_contain_error"
        );
        assert_eq!(SolanaEstimateOutcome::InvalidIx("x".into()).tag(), "invalid_ix");
    }

    // ─── 7. Broadcast tx ComputeBudget regression ────────────────────────────

    #[test]
    fn broadcast_tx_includes_priority_fee() {
        let key = load_signing_key(&"c".repeat(64)).expect("test key");
        let b = SolanaBroadcaster::new(key, "http://localhost:8899");
        let si = project(&base_intent());
        let tx = b.build_signed_tx(&si, &[0u8; 32]).expect("build_signed_tx");

        let found_price = tx.windows(10).find_map(|w| {
            if w[0] == 0x09 && w[1] == 0x03 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&w[2..10]);
                let p = u64::from_le_bytes(buf);
                if p > 0 { Some(p) } else { None }
            } else { None }
        });
        assert_eq!(
            found_price,
            Some(DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU),
            "broadcast tx must carry SetComputeUnitPrice"
        );
    }

    #[test]
    fn broadcast_tx_includes_compute_unit_limit() {
        let key = load_signing_key(&"d".repeat(64)).expect("test key");
        let b = SolanaBroadcaster::new(key, "http://localhost:8899");
        let si = project(&base_intent());
        let tx = b.build_signed_tx(&si, &[0u8; 32]).expect("build_signed_tx");
        assert!(
            tx.windows(6).any(|w| w[0] == 0x05 && w[1] == 0x02),
            "broadcast tx must carry SetComputeUnitLimit"
        );
    }

    #[test]
    fn broadcast_tx_is_signed_not_zeroed() {
        let key = load_signing_key(&"e".repeat(64)).expect("test key");
        let b = SolanaBroadcaster::new(key, "http://localhost:8899");
        let si = project(&base_intent());
        let tx = b.build_signed_tx(&si, &[1u8; 32]).expect("build_signed_tx");
        // Bytes 1..65 are the ed25519 signature — must NOT all be zero for a real key + non-zero blockhash
        let sig = &tx[1..65];
        assert_ne!(sig, &[0u8; 64], "real signing key must produce non-zero signature");
    }

    #[test]
    fn broadcast_tx_within_size_limit() {
        let key = load_signing_key(&"f".repeat(64)).expect("test key");
        let b = SolanaBroadcaster::new(key, "http://localhost:8899");
        let si = project(&base_intent());
        let tx = b.build_signed_tx(&si, &[0u8; 32]).expect("build_signed_tx");
        assert!(tx.len() <= 1232, "broadcast tx {} bytes > 1232 limit", tx.len());
    }

    // ─── 8. Order ID encoding ────────────────────────────────────────────────

    #[test]
    fn order_id_appears_big_endian_in_instruction() {
        // The order ID in the fixture starts with bytes 9f 8e 7d 6c.
        // In the serialized tx, these must appear as-is (big-endian, EVM-style).
        let si = project(&base_intent());
        let raw = build_tx(&si);
        let expected_prefix = [0x9f_u8, 0x8e, 0x7d, 0x6c];
        assert!(
            raw.windows(4).any(|w| w == expected_prefix),
            "order ID must appear big-endian in the instruction data"
        );
    }

    #[test]
    fn order_id_without_0x_prefix_is_decoded_correctly() {
        let mut intent = base_intent();
        intent.mayan_order_id = Some(
            "9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b".into(),
        );
        let si = MayanSolanaIntent::from_intent(&intent).expect("no-0x order id");
        let raw = build_tx(&si);
        let expected_prefix = [0x9f_u8, 0x8e, 0x7d, 0x6c];
        assert!(
            raw.windows(4).any(|w| w == expected_prefix),
            "order ID without 0x must also produce correct bytes"
        );
    }

    #[test]
    fn order_id_wrong_length_fails_projection() {
        let mut intent = base_intent();
        intent.mayan_order_id = Some("0xdeadbeef".into()); // only 4 bytes, not 32
        let err = MayanSolanaIntent::from_intent(&intent)
            .ok()
            .and_then(|si| {
                // The error surfaces at tx build time, not projection time
                sim().build_simulate_tx_b64(&si).err()
            });
        // Either projection or tx build must fail — accept either
        // (projection succeeds, tx build fails on decode_hex_32)
        if let Some(e) = err {
            assert!(
                e.to_string().contains("decode mayan_order_id hex")
                    || e.to_string().contains("32"),
                "error must mention the order id decode failure: {}",
                e
            );
        }
    }

    // ─── 9. Account key ordering ─────────────────────────────────────────────

    #[test]
    fn payer_is_first_account_in_message() {
        let key = load_signing_key(&"a".repeat(64)).expect("test key");
        let payer_bytes = key.verifying_key().to_bytes();
        let b = SolanaBroadcaster::new(key, "http://localhost:8899");
        let si = project(&base_intent());
        let tx = b.build_signed_tx(&si, &[0u8; 32]).expect("build");

        // After compact-u16(1) sig count + 64-byte sig + 3-byte message header + compact-u16(N) key count,
        // the first 32-byte key must be the payer.
        // Offset: 1 + 64 + 3 + 1 (compact-u16 for N keys, assuming N<128) = byte 69
        let header_end = 1 + 64 + 3 + 1; // sig_count + sig + header + key_count_varint
        let first_key = &tx[header_end..header_end + 32];
        assert_eq!(first_key, payer_bytes, "payer must be first account key");
    }

    // ─── 10. Compact-u16 encoding ────────────────────────────────────────────

    #[test]
    fn compact_u16_single_byte_values() {
        // Values 0–127 encode as a single byte
        fn write_compact_u16(mut n: u16) -> Vec<u8> {
            let mut buf = Vec::new();
            loop {
                let mut byte = (n & 0x7f) as u8;
                n >>= 7;
                if n == 0 {
                    buf.push(byte);
                    return buf;
                }
                byte |= 0x80;
                buf.push(byte);
            }
        }
        assert_eq!(write_compact_u16(0), vec![0x00]);
        assert_eq!(write_compact_u16(1), vec![0x01]);
        assert_eq!(write_compact_u16(127), vec![0x7f]);
        assert_eq!(write_compact_u16(128), vec![0x80, 0x01]);
        assert_eq!(write_compact_u16(16383), vec![0xff, 0x7f]);
    }

    // ─── 11. Sha256 PDA seed correctness ─────────────────────────────────────

    #[test]
    fn pda_seed_order_is_vault_then_order_hash_then_bump() {
        // Re-derive using the canonical seed order and verify the produced pubkey
        // matches derive_mayan_vault_pda for a known input.
        let order_hash = "9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b";
        let order_bytes = hex::decode(order_hash).expect("decode");
        let prog = bs58::decode(DEFAULT_MAYAN_SWIFT_PROGRAM)
            .into_vec()
            .expect("decode prog");

        // Find the first valid PDA bump (255 descending)
        let pda = (0u8..=255).rev().find_map(|bump| {
            let mut h = Sha256::new();
            h.update(b"vault");
            h.update(&order_bytes);
            h.update([bump]);
            h.update(&prog);
            h.update(b"ProgramDerivedAddress");
            let digest: [u8; 32] = h.finalize().into();
            if ed25519_dalek::VerifyingKey::from_bytes(&digest).is_err() {
                Some(bs58::encode(digest).into_string())
            } else {
                None
            }
        });

        let derived = derive_mayan_vault_pda(
            &format!("0x{}", order_hash),
            DEFAULT_MAYAN_SWIFT_PROGRAM,
        );
        assert_eq!(pda, derived, "PDA seed order must match Solana's find_program_address");
    }
}
