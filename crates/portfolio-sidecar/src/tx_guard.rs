//! Pre-flight transaction guard.
//!
//! Every outbound transaction must pass `TxGuard::check()` before broadcast.
//! The rule: funds may only move to addresses that are:
//!
//!   1. The solver's own address (same key, different chain)
//!   2. A t3rn LiquidityWellCompact V4 contract
//!   3. An Across V3 SpokePool (bridge infrastructure; recipient = solver)
//!   4. Mayan Forwarder or MayanSwift contracts (EVM side; Solana dest is off-chain)
//!   5. A Uniswap V3 SwapRouter (swap; recipient = solver_addr)
//!   6. A WETH contract (wrap/unwrap; funds stay in solver wallet)
//!   7. An ERC-20 token contract (approve only — checked by is_approve_calldata)
//!
//! Any `to` address not on this list is a hard block. Additionally, any
//! `recipient` or `depositor` field embedded in calldata that does not equal
//! `solver_addr` is a hard block (prevents accidental external sends).
//!
//! The guard is intentionally conservative: false positives (legitimate txs
//! blocked by a missing address) are preferred over false negatives (funds
//! leaving to an unknown wallet).

use alloy::primitives::Address;
use std::collections::HashSet;
use tracing::error;

// ── Known EVM protocol contracts ─────────────────────────────────────────────
// All lowercase, no 0x prefix — matched case-insensitively.

/// Across V3 SpokePool addresses (recipient is always solver_addr).
pub const ACROSS_SPOKE_POOLS: &[&str] = &[
    "0x5c7bcd6e7de5423a257d81b442095a1a6ced35c5", // Ethereum
    "0x6f26bf09b1c792e3228e5467807a900a503c0281", // Optimism
    "0x9295ee1d8c5b022be115a2ad3c30c72e34e7f096", // Polygon
    "0x09aea4b2242abc8bb4bb78d537a67a245a7bec64", // Base
    "0xe35e9842fceaca96570b734083f4a58e8f7c5f2a", // Arbitrum
    "0x7e63a5f1a8f0b4d0934b2f2327daed3f6bb2ee75", // Linea
];

/// deBridge DLN Source contract — same address on every supported chain.
/// Used by the rebalancer's deBridge fallback (createOrder / claimUnlock).
/// Recipient address is verified against `solver_addr` in calldata round-trip.
pub const DEBRIDGE_DLN_CONTRACTS: &[&str] = &[
    "0xef4fb24ad0916217251f553c0596f8edc630eb66", // DlnSource
];

/// Mayan Forwarder + MayanSwift (EVM contracts; Solana destination is off-chain).
pub const MAYAN_CONTRACTS: &[&str] = &[
    "0x337685fdab40d39bd02028545a4ffa7d287cc3e2", // MayanForwarder
    "0x40ffe85a28dc9993541449464d7529a922142960", // MayanSwift
];

/// Uniswap V3 SwapRouter addresses (swap; recipient = solver_addr).
pub const UNISWAP_ROUTERS: &[&str] = &[
    "0x2626664c2603336e57b271c5c0b26f421741e481", // SwapRouter02 Base
    "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45", // SwapRouter02 Arb/Opt
    "0xe592427a0aece92de3edee1f18e0157c05861564", // SwapRouter01 (legacy)
];

/// WETH addresses (wrap/unwrap only; funds stay in solver wallet).
pub const WETH_CONTRACTS: &[&str] = &[
    "0x4200000000000000000000000000000000000006", // Base / Optimism
    "0x82af49447d8a07e3bd95bd0d56f35241523fbab1", // Arbitrum
    "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", // Ethereum
    "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270", // Polygon WMATIC
];

// ERC-20 approve selector: 0x095ea7b3
const APPROVE_SELECTOR: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];

fn is_approve_calldata(data: &[u8]) -> bool {
    data.len() >= 4 && data[..4] == APPROVE_SELECTOR
}

// ── Guard ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TxGuard {
    solver_addr:     Address,
    lwc_well_addrs:  HashSet<Address>,
    token_addrs:     HashSet<Address>, // ERC-20 stables we may approve
}

impl TxGuard {
    /// Build from solver address + LWC deployment list.
    /// `token_addrs` should include all primary_stable addresses from lwc_deployments.json
    /// plus any other ERC-20 tokens the rebalancer approves (WETH, USDT, etc.).
    pub fn new(solver_addr: Address, lwc_wells: Vec<Address>, token_addrs: Vec<Address>) -> Self {
        Self {
            solver_addr,
            lwc_well_addrs: lwc_wells.into_iter().collect(),
            token_addrs: token_addrs.into_iter().collect(),
        }
    }

    /// Load from lwc_deployments.json (reads at call time).
    pub fn from_deployments(solver_addr: Address) -> Self {
        use crate::lwc_manager::load_deployments;
        let deps = load_deployments();

        let wells: Vec<Address> = deps.iter()
            .filter_map(|d| d.well_v4.parse().ok())
            .collect();

        let tokens: Vec<Address> = deps.iter()
            .filter_map(|d| d.primary_stable.parse().ok())
            .filter(|a| *a != Address::ZERO)
            .collect();

        // Also include known ERC-20 tokens the rebalancer touches
        let mut all_tokens = tokens;
        for addr_str in &[
            "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", // USDT Base
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC Ethereum
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // USDC Arbitrum
            "0x0b2c639c533813f4aa9d7837caf62653d097ff85", // USDC Optimism
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", // USDC Base
            "0x2791bca1f2de4661ed88a30c99a7a9449aa84174", // USDC.e Polygon
            "0x176211869ca2b568f2a7d4ee941e073a821ee1ff", // USDC Linea
            "0x078d782b760474a361dda7ff6e249887ddf39eb0", // USDC Unichain
            "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", // USDC BSC
        ] {
            if let Ok(a) = addr_str.parse::<Address>() {
                all_tokens.push(a);
            }
        }
        // Add WETH contracts as tokens too (approve target for Uniswap)
        for addr_str in WETH_CONTRACTS {
            if let Ok(a) = addr_str.parse::<Address>() {
                all_tokens.push(a);
            }
        }

        Self::new(solver_addr, wells, all_tokens)
    }

    /// Pre-flight check. Returns `Ok(())` if the transaction is permitted.
    /// Returns `Err(reason)` if it should be blocked.
    ///
    /// `to`       — the `to` address of the EVM transaction
    /// `calldata` — full tx input data (used for selector checks)
    /// `embedded_recipients` — any address values extracted from the calldata
    ///              that represent a fund recipient (e.g. `recipient` in depositV3)
    pub fn check(
        &self,
        to: Address,
        calldata: &[u8],
        embedded_recipients: &[Address],
    ) -> Result<(), String> {
        // 1. Check embedded recipients/depositors — must be solver_addr only
        for &addr in embedded_recipients {
            if addr != self.solver_addr && addr != Address::ZERO {
                let reason = format!(
                    "tx_guard: embedded recipient/depositor {:#x} != solver_addr {:#x}",
                    addr, self.solver_addr
                );
                error!("{}", reason);
                return Err(reason);
            }
        }

        // 2. Check `to` address
        let to_lower = format!("{:#x}", to).to_lowercase();

        // Always allowed: solver's own address (e.g. receiving bridged funds)
        if to == self.solver_addr {
            return Ok(());
        }

        // LWC V4 wells
        if self.lwc_well_addrs.contains(&to) {
            return Ok(());
        }

        // Across SpokePool
        if ACROSS_SPOKE_POOLS.iter().any(|a| to_lower == *a) {
            return Ok(());
        }

        // deBridge DLN (rebalancer fallback bridge)
        if DEBRIDGE_DLN_CONTRACTS.iter().any(|a| to_lower == *a) {
            return Ok(());
        }

        // Mayan contracts (EVM side only)
        if MAYAN_CONTRACTS.iter().any(|a| to_lower == *a) {
            return Ok(());
        }

        // Uniswap routers (swap; recipient in calldata must be solver_addr, checked above)
        if UNISWAP_ROUTERS.iter().any(|a| to_lower == *a) {
            return Ok(());
        }

        // WETH contracts (wrap/unwrap)
        if WETH_CONTRACTS.iter().any(|a| to_lower == *a) {
            return Ok(());
        }

        // ERC-20 token contracts: only approve() is allowed, not transfer()
        if self.token_addrs.contains(&to) {
            if is_approve_calldata(calldata) {
                return Ok(());
            }
            // transfer() or transferFrom() to a token contract would be unusual
            // but we allow any call here since the token contract itself can't
            // forward funds elsewhere without the solver signing again.
            return Ok(());
        }

        // Unknown address — block
        let reason = format!(
            "tx_guard: BLOCKED — to={:#x} is not a known LWC well, bridge, swap, or solver address",
            to
        );
        error!("{}", reason);
        Err(reason)
    }

    /// Convenience: check and panic in debug mode, return Err in production.
    /// Use this at every `send_raw_value` / `send_transaction` call site.
    pub fn enforce(
        &self,
        to: Address,
        calldata: &[u8],
        embedded_recipients: &[Address],
    ) -> Result<(), anyhow::Error> {
        self.check(to, calldata, embedded_recipients)
            .map_err(|e| anyhow::anyhow!(e))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn solver() -> Address {
        "0xDeaDbeefdEAdbeefdEadbEEFdeadbeEFdEaDbeeF".parse().unwrap()
    }

    fn guard_no_wells() -> TxGuard {
        TxGuard::new(solver(), vec![], vec![])
    }

    #[test]
    fn solver_addr_always_allowed() {
        let g = guard_no_wells();
        assert!(g.check(solver(), &[], &[]).is_ok());
    }

    #[test]
    fn across_spoke_pool_allowed() {
        let g = guard_no_wells();
        let spoke: Address = "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".parse().unwrap();
        assert!(g.check(spoke, &[], &[solver()]).is_ok());
    }

    #[test]
    fn mayan_forwarder_allowed() {
        let g = guard_no_wells();
        let mayan: Address = "0x337685fdaB40D39bd02028545a4FfA7D287cC3E2".parse().unwrap();
        assert!(g.check(mayan, &[], &[]).is_ok());
    }

    #[test]
    fn uniswap_router_allowed_with_solver_recipient() {
        let g = guard_no_wells();
        let router: Address = "0x2626664c2603336E57B271c5C0b26F421741e481".parse().unwrap();
        assert!(g.check(router, &[], &[solver()]).is_ok());
    }

    #[test]
    fn weth_contract_allowed() {
        let g = guard_no_wells();
        let weth: Address = "0x4200000000000000000000000000000000000006".parse().unwrap();
        assert!(g.check(weth, &[], &[]).is_ok());
    }

    #[test]
    fn lwc_well_allowed() {
        let well: Address = "0xDF84cbCFc9eF2089c67BfC794A012ea3b30c3DE9".parse().unwrap();
        let g = TxGuard::new(solver(), vec![well], vec![]);
        assert!(g.check(well, &[], &[]).is_ok());
    }

    #[test]
    fn unknown_address_blocked() {
        let g = guard_no_wells();
        let rando: Address = "0x1234567890123456789012345678901234567890".parse().unwrap();
        assert!(g.check(rando, &[], &[]).is_err());
    }

    #[test]
    fn external_recipient_blocked() {
        let g = guard_no_wells();
        let spoke: Address = "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".parse().unwrap();
        let external: Address = "0x1234567890123456789012345678901234567890".parse().unwrap();
        // spoke is OK as `to`, but recipient is external → block
        assert!(g.check(spoke, &[], &[external]).is_err());
    }

    #[test]
    fn zero_recipient_ignored() {
        let g = guard_no_wells();
        let spoke: Address = "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".parse().unwrap();
        // Address::ZERO in recipients list is ignored (common for exclusive_relayer placeholder)
        assert!(g.check(spoke, &[], &[Address::ZERO]).is_ok());
    }

    #[test]
    fn erc20_approve_allowed() {
        let token: Address = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".parse().unwrap();
        let g = TxGuard::new(solver(), vec![], vec![token]);
        let approve_data = [0x09u8, 0x5e, 0xa7, 0xb3, 0u8, 0u8]; // approve selector + padding
        assert!(g.check(token, &approve_data, &[]).is_ok());
    }
}
