# Protocol Support Matrix

Last updated: 2026-04-23

Generated from: `/Users/mbultra/projects/spinner/rust/crates/header-collector/protocols.xml`

## Executive Summary

- **Total Protocols**: 31
- **Bridge Protocols**: 21
- **Aggregators**: 4
- **Messaging Protocols**: 5
- **DEX Protocols**: 6 (analytics only)
- **Active with Volume**: 3 (T3RN LWC, LiFi V2, Stargate V2)
- **Total Chains Covered**: 38+

## Tier 1: High Priority (Active Markets)

Active protocols with recent fills and volume in the last 168 hours.

| Protocol | Category | Chains | Fee (bps) | SLA (ms) | Fills (7d) | Volume (7d USD) | Status |
|----------|----------|--------|-----------|----------|------------|-----------------|--------|
| **lifi_v2** | Aggregator | 20 | 4 | 20000 | 13 | $2,258.65 | ✅ ACTIVE |
| **t3rn_lwc** | Intent | 11 | 5 | 15000 | 7,367 | $0 | ✅ ACTIVE (Lambda) |
| **stargate_v2** | Bridge | 17 | 2 | 12000 | 6 | $0 | ✅ ACTIVE |

### Notes
- **LiFi V2**: Top revenue generator ($0.90 estimated). Observed fee spikes (49-10000 bps vs 4 bps default).
- **T3RN LWC**: High fill count (7,367) but $0 USD tracking (Lambda tracked separately). 73.7% fill rate.
- **Stargate V2**: Low fees (1-2 bps), LayerZero-based OFT bridge.

## Tier 2: Medium Priority (Dormant but Indexed)

Protocols fully supported by Spinner but with zero activity in last 7 days.

| Protocol | Category | Chains | Fee (bps) | SLA (ms) | Priority | Status |
|----------|----------|--------|-----------|----------|----------|--------|
| **across_v3** | Bridge | 15 | 3 | 10000 | 1 | 📋 Ready |
| **debridge_dln** | Bridge | 13 | 4 | 8000 | 1 | 📋 Ready |
| **relay_protocol** | Bridge | 10 | 3 | 10000 | 1 | 📋 Ready |
| **hop_protocol** | Bridge | 8 | 2 | 15000 | 2 | 📋 Ready |
| **cctp** | Bridge | 8 | 2 | 15000 | 2 | 📋 Ready |
| **connext** | Bridge | 10 | 2 | 20000 | 2 | 📋 Ready |
| **synapse** | Bridge | 12 | 3 | 20000 | 2 | 📋 Ready |
| **celer_cbridge** | Bridge | 10 | 2 | 20000 | 2 | 📋 Ready |
| **mayan_swift** | Bridge | 6 | 5 | 30000 | 2 | 📋 Ready |
| **wormhole** | Bridge | 12 | 2 | 60000 | 2 | 📋 Ready |
| **meson_finance** | Bridge | 10 | 2 | 30000 | 2 | 📋 Ready |
| **allbridge** | Bridge | 8 | 3 | 25000 | 2 | 📋 Ready |
| **router_protocol** | Bridge | 10 | 4 | 20000 | 2 | 📋 Ready |
| **symbiosis** | Bridge | 8 | 3 | 20000 | 2 | 📋 Ready |
| **socket** | Aggregator | 12 | 3 | 20000 | 2 | 📋 Ready |
| **squid_router** | Aggregator | 15 | 2 | 20000 | 2 | 📋 Ready |
| **rango** | Aggregator | 8 | 3 | 20000 | 3 | 📋 Ready |

## Tier 3: Messaging Protocols

Cross-chain messaging infrastructure (not direct bridges).

| Protocol | Category | Chains | Fee (bps) | SLA (ms) | Priority | Status |
|----------|----------|--------|-----------|----------|----------|--------|
| **layerzero_v2** | Messaging | 15 | 2 | 30000 | 2 | 📋 Ready |
| **hyperlane** | Messaging | 12 | 2 | 30000 | 2 | 📋 Ready |
| **axelar_gmp** | Messaging | 14 | 3 | 30000 | 2 | 📋 Ready |
| **ccip** | Messaging | 10 | 3 | 30000 | 2 | 📋 Ready |

## Tier 4: Native Bridges

Official L1-L2 bridges (low priority for solver).

| Protocol | Category | Chains | Fee (bps) | SLA (ms) | Priority | Status |
|----------|----------|--------|-----------|----------|----------|--------|
| **arbitrum_native** | Native Bridge | 2 | 0 | 900000 | 3 | 📋 Ready |
| **optimism_native** | Native Bridge | 2 | 0 | 900000 | 3 | 📋 Ready |

## Tier 5: DEX Protocols (Analytics Only)

Not cross-chain, tracked for swap analysis.

| Protocol | Category | Chains | Fee (bps) | Status |
|----------|----------|--------|-----------|--------|
| **1inch_fusion** | DEX | 5 | 30 | 🔍 Analytics |
| **bancor** | DEX | 1 | 30 | 🔍 Analytics |
| **uniswap_v2** | DEX | 9 | 30 | 🔍 Analytics |
| **uniswap_v3** | DEX | 6 | 30 | 🔍 Analytics |
| **uniswap_v4** | DEX | 1 | 30 | 🔍 Analytics |

## Chain Coverage Matrix

Protocols indexed per chain:

| Chain ID | Chain Name | Protocol Count | Top Protocols |
|----------|------------|----------------|---------------|
| 1 | Ethereum | 31 | All protocols |
| 10 | Optimism | 25 | LiFi, Across, Stargate, Hop, Connext |
| 8453 | Base | 22 | LiFi, Across, Stargate, Relay, CCTP |
| 42161 | Arbitrum | 26 | LiFi, Across, Stargate, deBridge, Mayan |
| 137 | Polygon | 21 | LiFi, Stargate, Hop, Hyperlane, CCTP |
| 56 | BSC | 20 | LiFi, Stargate, Wormhole, Celer, Synapse |
| 43114 | Avalanche | 18 | LiFi, Stargate, Wormhole, CCTP |
| 250 | Fantom | 16 | LiFi, Stargate, Wormhole, Synapse |
| 100 | Gnosis | 15 | LiFi, Hop, Connext, Synapse |
| 324 | zkSync Era | 19 | LiFi, Stargate, Relay, Connext |
| 59144 | Linea | 16 | LiFi, Stargate, Hop, CCTP |
| 534352 | Scroll | 14 | LiFi, Stargate, Synapse |
| 81457 | Blast | 11 | LiFi, Stargate, T3RN, Relay |
| 252 | Fraxtal | 9 | LiFi, Stargate, Axelar |
| 169 | Manta | 7 | LiFi, Stargate, Squid |
| 200 | Solana | 3 | Mayan, Wormhole, Allbridge |
| 637 | Aptos | 2 | Wormhole, Allbridge |
| 500 | Tron | 1 | Wormhole |
| 143 | Monad | 3 | LiFi, T3RN |
| 1329 | Sei | 4 | LiFi, Stargate, T3RN |

## Event Signatures Reference

### Deposit Events (Topic0)

Most common deposit event signatures across protocols:

```
Across V3 FundsDeposited:
  0x32ed1a409ef04c7b0227189c3a103dc5ac10e775a15b785dcc510201f7c25ad3
  0xa123dc29aebf7d0c3322c8eeb5b999e859f39937950ed31056532713d0de396f

Stargate V2 OFTSent:
  0x85496b760a4b7f8d66384b9df21b381f5d1b1e79f229a47aaf4c232edc2fe59a

LiFi TransferStarted:
  0xcba69f43792f9f399347222505213b55af8e0b0b54b893085c2e27ecbe1644f1
  0xcba69f43792fcd97be2cec30c47f08aa73f30e9e5640af82e3baf888b29f12b3

deBridge CreatedOrder:
  0xfc8703fd57380f9dd234a89dce51333782d49c5902f307b02f03e014d18fe471

Mayan ForwardedERC20:
  0xbf150db6b4a14b084f7346b4bc300f552ce867afe55be27bce2d6b37e3307cda

CCTP DepositForBurn:
  0x2fa9ca894982930190727e75500a97d8dc500233a5065e0f3126c48fbe0343c0

T3RN OrderCreated:
  0x1e5f09566c518bd8ddb1f0c3fd1da318e49162cf94b58c4508d2835fc971e60d
  0x3bb399125b923176baf5098f432689e4843dee54b68daf1d7cadd91d99a63601
```

### Fill Events (Topic0)

Most common fill event signatures:

```
Across V3 FilledRelay:
  0x44b559f101f8fbcc8a0ea43fa91a05a729a5ea6e14a7c75aa750374690137208
  0x571749edf1d5c9599318cdbc4e28a6475d65e87fd3b2df5ff0de7b3a5b4d72de

Stargate V2 OFTReceived:
  0xefed6d3500546b29533b128a29e3a94d70788727f0507505ac12eaf2e578fd9c

deBridge FulfilledOrder:
  0x68f99a6de5e9d34cce40430ee30c8b4ad2cb0f7e6fda52c9e8f10f9c35fcd5df

Mayan OrderFulfilled:
  0x8fb2c26b66af59de39b1b2f4e1fba157f4408a9b52495599333e37e3191b0869

T3RN Fill:
  0xba78a15e874441cf1871e3d2633ba91540bab663ae8664088ace7d60009ddd65
```

## Integration Priority for Solver

### Immediate (Week 1)
1. **LiFi V2** - Active volume leader ($2,258 in 7d)
2. **T3RN LWC** - High fill count (7,367), Lambda integration
3. **Stargate V2** - Active, low fees

### Short-term (Week 2-4)
4. **Across V3** - Sub-10s finality, 15 chains
5. **deBridge DLN** - 8s SLA, intent-based
6. **Relay** - Fast bridge, 10 chains

### Medium-term (Month 2)
7. **Hop Protocol** - L2 specialist
8. **CCTP** - Native USDC (Circle)
9. **Connext** - Modular bridge
10. **Synapse** - Cross-chain swaps

### Long-term (Month 3+)
- Messaging protocols (LayerZero, Hyperlane, Axelar, CCIP)
- Remaining bridges (Mayan, Wormhole, Celer, etc.)
- Native bridges (Arbitrum, Optimism)

## Fee Analysis

### Fee Tiers by Protocol

| Fee Tier | Protocols | Description |
|----------|-----------|-------------|
| **0 bps** | Arbitrum Native, Optimism Native | No fees (gas only) |
| **2 bps** | Stargate V2, Hop, CCTP, Connext, Celer, Wormhole, Hyperlane, LayerZero, Meson, Squid | Ultra-low fee tier |
| **3 bps** | Across V3, Relay, Synapse, Allbridge, Socket, Axelar, CCIP, Rango, Symbiosis | Low fee tier |
| **4 bps** | LiFi V2, deBridge, Router Protocol | Medium fee tier |
| **5 bps** | T3RN LWC, Mayan | Higher fee tier |
| **30 bps** | All DEX protocols | DEX standard |

### Fee Anomalies Detected (Last 7d)

From `solver_intel.json`:

- **LiFi V2**: Observed fees ranging from 49 bps to 10,000 bps (default: 4 bps)
  - Spike solver: `0x2e2e5c79...d30570` (10,000 bps)
  - High solver: `0x64739640...71fb45` (529 bps)
  - Normal solver: `0x47fa0c6b...df0497` (49 bps)

**Recommendation**: Monitor LiFi V2 fee structure closely. May be user-set slippage tolerance rather than protocol fee.

## Latency Analysis

### SLA Tiers

| SLA Range | Protocols | Description |
|-----------|-----------|-------------|
| **1-10s** | DEX protocols, Across V3, deBridge, Relay | Ultra-fast |
| **10-20s** | Stargate V2, Hop, CCTP, LiFi, Socket, Squid, Connext, Synapse, Celer, Router, Symbiosis | Fast |
| **20-30s** | Mayan, Meson, Hyperlane, LayerZero, Axelar, CCIP | Medium |
| **60s+** | Wormhole | Slow |
| **15 min** | Native bridges | Very slow (finality) |

### Observed Latency (Last 7d)

From `solver_intel.json`:

- **LiFi V2 average**: 55.4s (exceeds 20s SLA)
  - Longest: `0x33377bf8...2e89a3` → 206s (Ethereum → Sei)
  - Longest: `0x8c2c3117...0497aa` → 126s (Optimism → Gnosis)

**Note**: High latency likely due to destination chain finality (Sei, Gnosis).

## Contract Addresses

### Primary Contracts by Chain

**Ethereum (1):**
```
Across V3:        0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5
Stargate V2:      0x77b2043768d28E9C9aB44E1aBfC95944bcE57931
LiFi:             0x1231DEB6f5749EF6cE6943a275A1D3E7486F4EaE
deBridge:         0xeF4fB24aD0916217251F553c0596F8Edc630EB66
Mayan:            0x337685fdab40d39bd02028545a4ffa7d287cc3e2
Wormhole:         0x98f3c9e6E3fAce36bAAd05FE09d375Ef1464288B
Hyperlane:        0xc005dc82818d67AF737725bD4bf75435d065D239
Hop:              0xb8901acB165ed027E32754E0FFe830802919727f
Connext:          0x8898B472C54c31894e3B9bb83cEA802a5d0e63C6
Relay:            0xa5F565650890Fba1824Ee0F21EbbBbdf3c4D0381
CCTP:             0xBd3fa81B58Ba92a82136038B25aDec7066af3155
Arbitrum Native:  0x72Ce9c846789fdB6fC1f34aC4AD25Dd9ef7031ef
Optimism Native:  0x99C9fc46f92E8a1c0deC1b1747d010903E884bE1
Celer:            0x5427FEFA711Eff984124bFBB1AB6fbf5E3DA1820
Synapse:          0x2796317b0fF8538F253012862c06787Adfb8cEb6
LayerZero V2:     0x1a44076050125825900e736c501f859c50fE728c
Meson:            0x25aB3Efd52e6f14225146D740466ae7986f4B434
Allbridge:        0x1231deb6f5749ef6ce6943a275a1d3e7486f4eae
Router:           0x6e14f48576265272B6CAA3a886F678602eCA1C22
Symbiosis:        0xb8f275fBf7A959F4BCE59999A2EF122A099e81A8
CCIP:             0xe7be8aff3b99786fc77d4430e4152c63c868d453
Socket:           0x3a23F943181408EAC424116Af7b7790c94Cb97a5
Squid:            0xce16F69375520ab01377ce7B88f5BA8C48F8D666
1inch:            0x1111111254EEB25477B68fb85Ed929f73A960582
Bancor:           0xeEF417e1D5CC832e619ae18D2F140De2999dD4fB
Uniswap V2:       0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc
Uniswap V3:       0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640
Uniswap V4:       0xE8E23e97Fa135823143d6b9Cba9c699040D51F70
```

**Note**: Most protocols deploy same contract address across all chains (CREATE2). Verify on-chain before execution.

## Solver Opportunities

### Revenue Potential (Last 7d)

From `solver_intel.json`:

| Protocol | Fills | Volume USD | Est Revenue USD | Revenue/Fill |
|----------|-------|------------|-----------------|--------------|
| LiFi V2 | 13 | $2,258.65 | $0.90 | $0.069 |
| Stargate V2 | 6 | $0 | $0 | $0 |
| T3RN LWC | 7,367 | $0 | $0 | $0 |

**Key Insights:**
- LiFi V2 is the only protocol generating trackable revenue
- High fee variance suggests market inefficiency
- T3RN LWC has high volume but zero USD tracking (Lambda orders)

### Active Solver Count

- **LiFi V2**: 13 unique solvers
- **Stargate V2**: 5 unique solvers
- **T3RN LWC**: 1 solver (t3rn official)

**Market Entry**: LiFi V2 and Stargate V2 are competitive markets. Consider T3RN LWC integration for first-mover advantage.

## Next Steps

1. **Immediate**: Implement LiFi V2 intent parsing and profit calculation
2. **Week 1**: Add Stargate V2 and T3RN LWC support
3. **Week 2**: Test against live deposits on Base + Arbitrum
4. **Week 3**: Add Across V3, deBridge, Relay
5. **Month 2**: Expand to remaining Tier 2 protocols
6. **Month 3**: Messaging protocol integration for advanced routing

## Solver Reward / Repayment Mechanics

Last verified: 2026-04-28 (after first live fill on Base).

### Across V3
- **Fill call**: `fillRelay(relayData, repaymentChainId, repaymentAddress)` on destination SpokePool
- **Repayment**: **Automatic / trustless** — no claim call needed. Across's off-chain dataworker bundles all fills into a Merkle root every ~1.5h, submits to HubPool on Ethereum, challenge window passes, then HubPool pushes repayment to relayer on `repaymentChainId` at `repaymentAddress`.
- **What we receive**: `inputAmount` of the deposit's input token on the repayment chain. Fee = `inputAmount - outputAmount`.
- **Important**: Set `repaymentChainId = destinationChainId` (same chain you filled on) so repayment lands where you have gas and reusable liquidity. Setting it to origin chain means repayment arrives on a chain where you may have no gas (as happened with our first fill — repaymentChain=Polygon, we have 0 MATIC there).
- **Solver code status**: Filler sets `repaymentChainId = DEST` as of 2026-04-28. First confirmed fill: tx `0x262b9d65...` on Base block 45298537.

### deBridge DLN
- **Fill call**: `fulfillOrder()` on DlnDestination contract (destination chain)
- **Repayment**: **Manual** — after filling, solver must call `claimUnlock()` on DlnSource contract on the **source chain** to receive the locked input tokens.
- **What we receive**: The order's input token on source chain.
- **Solver code status**: `claim_funds()` exists in `crates/protocol-adapters/src/debridge.rs` but is stub-only (`"Live deBridge fill execution not yet implemented"`). `claimUnlock()` call is **not wired up**.

### LiFi V2
- **Fill call**: LiFi is a router/aggregator — it calls underlying bridges (Across, Stargate, etc.). No proprietary LiFi fill function.
- **Repayment**: **Automatic** — cost/reward is baked into the spread quoted to the user. No separate claim.
- **Solver code status**: `crates/protocol-adapters/src/lifi.rs` handles routing; no separate claim step needed.

### Mayan Finance
- **Fill call**: Solver wins an on-chain auction and executes the swap atomically via Wormhole.
- **Repayment**: **Automatic** — profit is captured atomically within the swap execution (auction spread).
- **Solver code status**: Mayan adapter exists; claim is N/A.

### Taifoon Operator path (Linea, Arbitrum — `operator != 0x0`)
- **Fill call**: `executeWithProof(v5ProofBlob, adapterContract, adapterCalldata)` on TaifoonUniversalOperator
- **Repayment**: **Manual** — must call `claim()` on the operator contract after fill is confirmed to release solver fee.
- **Solver code status**: `lambda_claim` in `HACKATHON_COLOSSEUM_PLAN.md` describes this step. **Not yet wired in production** — only stubbed in `lambda_controller.rs`. This path has not been exercised in live fills yet.

### Summary

| Protocol | Claim needed? | Where repaid | Status |
|---|---|---|---|
| Across V3 (direct, Base/Optimism) | ❌ Automatic | repaymentChain (set to dest) | ✅ Live |
| deBridge DLN | ✅ `claimUnlock()` on src chain | Source chain, input token | 🔴 Not wired |
| LiFi V2 | ❌ Automatic (spread) | N/A | ✅ (router only) |
| Mayan Finance | ❌ Automatic (auction) | Atomic in swap | ⚠️ Untested |
| Taifoon Operator (Linea/Arb) | ✅ `claim()` on operator | Operator contract | 🔴 Not wired |

## References

- **Source XML**: `/Users/mbultra/projects/spinner/rust/crates/header-collector/protocols.xml`
- **Registry JSON**: `/Users/mbultra/projects/taifoon-solver/config/protocols_registry.json`
- **Intel JSON**: `/Users/mbultra/projects/taifoon-solver/config/solver_intel.json`
- **Spinner DA API**: `http://46.4.96.124:8081/api/v5/proof/blob/:chain_id/:block_number`
