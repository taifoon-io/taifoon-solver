use alloy::primitives::Address;
use std::collections::HashMap;

pub struct LWCConfig {
    pub contracts: HashMap<u64, Address>, // chain_id -> LWC address
}

impl LWCConfig {
    pub fn new() -> Self {
        let mut contracts = HashMap::new();

        // TODO: Get actual deployed addresses from t3rn-guardian/src/config/config.ts
        // These are placeholders - need to be updated with real addresses

        // Base Sepolia (testnet)
        contracts.insert(
            84532,
            "0x0000000000000000000000000000000000000000".parse().unwrap(),
        );

        // Optimism Sepolia (testnet)
        contracts.insert(
            11155420,
            "0x0000000000000000000000000000000000000000".parse().unwrap(),
        );

        // Base Mainnet
        contracts.insert(
            8453,
            "0x0000000000000000000000000000000000000000".parse().unwrap(),
        );

        // Optimism Mainnet
        contracts.insert(
            10,
            "0x0000000000000000000000000000000000000000".parse().unwrap(),
        );

        Self { contracts }
    }

    pub fn get_contract(&self, chain_id: u64) -> Option<Address> {
        self.contracts.get(&chain_id).copied()
    }
}
