# Solver Loops — Full Cycle Documentation

> Status as of 2026-04-28. All 4 processes running.

---

## 1. Across V3 — USDC + WETH fills on Base

### What we can fill
| Token | Our inventory | Trigger |
|-------|--------------|---------|
| USDC (any chain) | 7.96 USDC on Base | `FundsDeposited` → outputToken in USDC_ADDRESSES |
| WETH (any chain) | ~0.004 WETH on Base | `FundsDeposited` → outputToken in WETH_ADDRESSES |

### Full loop
```
1. Watch FundsDeposited on ETH/OP/ARB/BSC/POLYGON SpokePools (WS)
   topic: 0x44b559f101f8fbcc8a0ea43fa91a05a729a5ea6e14a7c75aa750374690137208

2. Decode event (non-indexed data layout, deployed contracts):
   word0: inputToken    word1: outputToken    word2: inputAmount
   word3: outputAmount  word4: dstChainId     word5: fillDeadline
   word6: quoteTimestamp  word7: exclusivityDeadline
   word8: recipient     word9: exclusiveRelayer  word10+: message

3. Filter:
   - destinationChainId == 8453 (Base)
   - outputToken in USDC_ADDRESSES or WETH_ADDRESSES
   - fillDeadline > now
   - exclusivityDeadline < now  (skip if still exclusive)
   - balance >= outputAmount

4. Send fillRelay on Base SpokePool (0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64)
   selector: 0xdeff4b24
   fillRelay(RelayData relayData, uint256 repaymentChainId, bytes32 updatedRecipient)
   RelayData = (depositor, recipient, exclusiveRelayer, inputToken, outputToken,
                inputAmount, outputAmount, originChainId, depositId,
                fillDeadline, exclusivityDeadline, message)
   repaymentChainId = 8453 (Base)
   updatedRecipient = our solver address

5. Gas: eth_estimateGas × 1.3 buffer. actual ~104k gas for ERC20 fill.
   maxFeePerGas = max(gasPrice*2, 1 gwei)
   priorityFee  = min(1 gwei, maxFeePerGas)

6. On fill success: Across relayer pool credits us on Base automatically.
   No manual claim needed — repaymentChainId handles it.
```

### USDC address map
- Base:     `0x833589fcd6edb6e08f4c7c32d4f71b54bda02913`
- ETH:      `0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48`
- Arbitrum: `0xaf88d065e77c8cc2239327c5edb3a432268e5831`
- Optimism: `0x0b2c639c533813f4aa9d7837caf62653d097ff85`
- Polygon:  `0x3c499c542cef5e3811e1192ce70d8cc03d5c3359`
- BSC:      `0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d`

### WETH address map
- Base/Optimism: `0x4200000000000000000000000000000000000006`
- ETH:           `0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2`
- Arbitrum:      `0x82af49447d8a07e3bd95bd0d56f35241523fbab1`
- Polygon:       `0x7ceb23fd6bc0add59e62ac25578270cff1b9f619`

### Confirmed fills
- deposit 5619261: 0.001982 WETH, Optimism→Base, block 45305649 ✅
- deposit 5619247: 0.000594 WETH, Optimism→Base, block 45305685 ✅

### Blockers
- USDC balance 7.96 — all USDC deposits 8.98+ so we skip. Need to top up.
- ETH on Base ~0.0004 — barely covers gas. Keep topped up.
- Old Polygon/BSC SpokePool puts an EVM address at word7 instead of uint32 exclusivityDeadline — guarded with `if excl_deadline_raw < (1<<33) else 0`.

---

## 2. deBridge DLN — EVM→EVM full cycle

### What we can fill
EVM→EVM open (non-exclusive) orders where take token is USDC/USDT on our supported chains.

Supported take chains/tokens:
| Chain | Tokens |
|-------|--------|
| 8453 Base | USDC |
| 42161 Arbitrum | USDC, USDT |
| 10 Optimism | USDC, USDT |
| 1 Ethereum | USDC, USDT |

### Full loop — EVM→EVM
```
1. Watch CreatedOrder on DlnSource (0xeF4fB24aD0916217251F553c0596F8Edc630EB66)
   topic: 0xfc8703fd57380f9dd234a89dce51333782d49c5902f307b02f03e014d18fe471
   event has NO indexed params except sig — everything is in data

2. Decode event inline (no API call needed):
   ABI decode: (Order, bytes32 orderId, bytes affiliateFee, uint256 nativeFee,
                uint256 percentFee, uint32 refCode, bytes metadata)
   Order tuple fields (14 total):
     [0] makerOrderNonce uint64
     [1] makerSrc        bytes
     [2] giveChainId     uint256
     [3] giveTokenAddress bytes
     [4] giveAmount      uint256
     [5] takeChainId     uint256   ← dst chain
     [6] takeTokenAddress bytes
     [7] takeAmount      uint256
     [8] receiverDst     bytes
     [9] givePatchAuthority bytes
     [10] orderAuthorityDst bytes
     [11] allowedTakerDst bytes    ← if non-zero, exclusive
     [12] allowedCancelBeneficiary bytes
     [13] externalCall   bytes

3. Quick-skip inline:
   - takeChainId == 7565164 → EVM→Solana, skip
   - takeChainId not in CHAIN_HTTP → unsupported chain, skip
   - allowedTakerDst non-zero and != our address → exclusive, skip
   - takeToken not in SUPPORTED_TOKENS[takeChainId] → skip

4. Verify orderId = keccak256(packed Order fields)

5. Check balance: get_erc20_balance(takeToken, SOLVER_ADDR) >= takeAmount

6. Read takePatches(bytes32 orderId) from DlnDestination on dst chain
   selector: keccak256("takePatches(bytes32)")[:4]
   fulFillAmount = takeAmount - takePatches

7. Call fulfillOrder on DlnDestination (0xE7351Fd770A37282b91D153Ee690B63579D6dd7f)
   selector: 0xc358547e
   fulfillOrder(Order order, uint256 fulFillAmount, bytes32 orderId,
                bytes permitEnvelope, address unlockAuthority)
   value = 0 (ERC20 fill)

8. After confirmation, call sendEvmUnlock to release src tokens:
   sendEvmUnlock(bytes32 orderId, address beneficiary, uint256 executionFee)
   selector: 0xb41100b3
   value = globalFixedNativeFee + executionFee * 10000 / (10000 - globalTransferFeeBps)
   executionFee = 0.0005 ETH (keeper incentive)
   Query fees from deBridgeGate: globalFixedNativeFee(), globalTransferFeeBps()

9. claimUnlock fires automatically — deBridge keepers call it via deBridgeGate.callProxy
   after sendEvmUnlock tx is confirmed + validators sign.
   Source tokens released to beneficiary address on src chain.
```

### DLN contract addresses
| Contract | Address | All EVM chains |
|----------|---------|----------------|
| DlnSource | `0xeF4fB24aD0916217251F553c0596F8Edc630EB66` | ETH/OP/ARB/BSC |
| DlnDestination | `0xE7351Fd770A37282b91D153Ee690B63579D6dd7f` | ETH/OP/ARB/BSC |
| deBridgeGate | `0x43dE2d77BF8027e25dBD179B491e8d64f38398aA` | ETH/OP/ARB/BSC |
| deBridgeGate | `0xc1656B63D9EEBa6d114f6bE19565177893e5bCBF` | Base |

### Blockers
- ~100% of live orders are EVM→Solana (takeChainId=7565164)
- Remaining EVM→EVM orders are almost all exclusive (`allowedTakerDst` set to `0x555ce236...` — a known large MM)
- **Action required**: contact deBridge team to register as a market maker to receive non-exclusive orders or be whitelisted on exclusive routes.
- deBridge Discord: https://discord.gg/debridge

---

## 3. deBridge DLN — Solana-side fill (EVM→Solana orders)

### What this enables
Fill orders where the user sends from EVM and receives on Solana. We receive SOL/SPL tokens on Solana after filling, and the src chain EVM tokens unlock to our EVM address.

### Solana program
- **DLN Destination program**: `dst5MGcFPoBeREFAA5E3tU5ij8m5uVYwkzkSAbsLbNo`
- **deBridge program**: `DEbrdGj3HsRsAzx6uH4MKyREKxVAfBydijLUF3ygsFfh`
- **Settings PDA**: `DeSetTwWhjZq6Pz9Kfdo1KoS5NqtsM6G8ERbX4SSCSft`

### Full loop — EVM→Solana
```
1. Same WS watch on src EVM chain for CreatedOrder events
   Decode inline: takeChainId == 7565164 means dst is Solana

2. takeTokenAddress is a Solana pubkey encoded in 32 bytes
   takeAmount in SPL token lamports

3. Fill instruction: fulfillOrder on dst5MGcFPoBeREFAA5E3tU5ij8m5uVYwkzkSAbsLbNo
   Required accounts:
   - takeOrderState (writable, PDA): seeds = ["TAKE_ORDER_STATE", orderId]
   - taker (writable, signer): our Solana keypair
   - takerWallet (writable): our SPL token ATA for takeToken
   - receiverDst (readable): receiver's token account
   - authorizedSrcContract (PDA): seeds = ["AUTHORIZED_SRC_CONTRACT", giveChainId]
   - takeOrderPatch (readable, PDA)
   - splTokenProgram, systemProgram

4. Arguments:
   - unvalidatedOrder: full Order struct (same 14 fields, Solana encoding)
   - orderId: [u8; 32]
   - unlockAuthority: Option<Pubkey> — our EVM address encoded or None to use default

5. After Solana fill confirms, call sendSolanaUnlock (or equivalent) to release
   EVM src tokens — deBridge validators observe Solana state and auto-unlock.

6. EVM tokens released to our EVM beneficiary address.
```

### What we need to implement this
- Solana keypair with SPL token accounts for supported tokens (USDC/USDT/native SOL)
- @solana/web3.js + @debridge-finance/dln-client SDK or raw instruction building
- The IDL is at `/tmp/dln-client-pkg/package/dist/types/solana/idl/dst.d.ts`
- Min example: `dlnDst.fulfillOrder(order, takerPublicKey, unlockAuthorityPubkey, takerTokenWallet)`

### Status
- `mayan_watcher.py` currently tracks orders on EVM chains (read-only)
- **Action required**: create `/tmp/debridge_solana_filler.py` with solana-py or anchor-client

---

## 4. Mayan Swift — EVM→Solana + Solana→EVM

### Architecture
Mayan Swift uses a **3-second English auction on Solana** before any fill. The auction winner gets a Wormhole VAA which they present on the destination chain to fill.

```
User (EVM) → MayanForwarder.forwardEth / forwardERC20
           → Wormhole message published
           → Solana auction program receives message
           → 3-second open auction among registered drivers
           → Auction winner signs → Wormhole VAA issued
           → Winner fills on destination:
               EVM dst: MayanSwift.unlockSingle(bytes encodedVm)
               SOL dst: call Mayan Swift Solana program with VAA

Unlock flow:
  Filler → presents VAA → destination tokens released to user
         → source tokens unlocked to filler (Wormhole + Mayan keepers)
```

### Contracts watched
| Contract | Chain | Topic |
|----------|-------|-------|
| MayanForwarder `0x337685fdab40d39bd02028545a4ffa7d287cc3e2` | ETH/OP/ARB/BSC/POLY | SwapAndBridge |
| MayanSwift `0xC38e4e6A15593f908255214653d3D947CA1c2338` | ETH/OP/ARB/BSC/POLY | OrderCreated |

### EVM fill (Solana auction → EVM dst)
```
Fill selector: unlockSingle(bytes encodedVm)
Contract: MayanSwift on destination EVM chain
Requires: Wormhole VAA from completed Solana auction

The VAA contains:
  - trader, tokenOut, minAmountOut, gasDrop
  - cancelFee, refundFee, deadline
  - destAddr, destChainId
  - auctionMode, random
```

### Solana fill (EVM src → Solana dst)
```
Mayan Swift Solana program: auction + fulfillment
After winning auction, call fulfillAuction with VAA
Destination: user receives SPL tokens on Solana
```

### What we need to participate
1. **Driver registration** — Mayan requires SDK access via Discord. No permissionless fill.
   - Join: https://discord.gg/mayanswap → #become-a-driver channel
   - Request access to `@mayanfinance/swap-sdk` or `mayan-solver-sdk`

2. **Solana keypair** — participate in the 3s English auction bidding
3. **Multicall / latency** — must bid and win within the 3-second window
4. **Capital on Solana** — need USDC/USDT on Solana to fill SOL-destined orders

### Status
- `mayan_watcher.py` (PID 3516): read-only observer, ~10+ orders tracked
- 0 fills possible without driver registration
- **Action required**: contact Mayan via Discord for driver SDK access

---

## Current Process Status

| Process | PID | Status | Last event |
|---------|-----|--------|-----------|
| `across_ws_filler.py` | 43937 | ✅ LIVE | WETH+USDC fills + ETH mempool watcher |
| `debridge_filler_v2.py` | 22745 | ✅ LIVE | EVM→Solana orders correctly identified |
| `mayan_watcher.py` | 3516 | ✅ LIVE (read-only) | Orders observed |
| `across_direct_filler.sh` | 88967 | ✅ LIVE | Backup poller |

## Confirmed fills
| Protocol | Deposit | Amount | Route | Block | Tx |
|----------|---------|--------|-------|-------|----|
| Across | 5619261 | 0.001982 WETH | OP→Base | 45305649 | 18446d5c... |
| Across | 5619247 | 0.000594 WETH | OP→Base | 45305685 | 808c6481... |
| Across | 5619324 | 0.002608 WETH | OP→Base | 45306342 | ab6e4a4c... |

## Bugs fixed this session
- ETH WETH address was wrong: `0xc02aaa39b223fe8d0a057b83...` → `0xc02aaa39b223fe8d0a0e5c4f...`
- deBridge event decode used `topics[1]` (doesn't exist) → inline ABI decode from `data`
- Gas: `maxPriorityFee > maxFee` on Base (low base fee) → `priority = min(1 gwei, max_fee)`
- Skip on estimateGas revert (AlreadyFilled) → saves ~74k gas per missed deposit

## Inventory as of 2026-04-28 20:47 UTC
| Token | Have | Notes |
|-------|------|-------|
| WETH Base | 0.00205 | Can fill up to ~0.002 WETH deposits |
| USDC Base | 2.96 | Too low — Across repayment pending (~1h) |
| ETH Base | 0.00159 | Gas for ~15 fill attempts |
| SOL + USDC (Solana) | 0 | Need inventory for deBridge EVM→Solana |
| Mayan driver SDK | not registered | contact #become-a-driver on Discord |

## Key findings — deBridge
- 100% of live orders are exclusive (`allowedTakerDst` set to known MMs)
- Both EVM→EVM and EVM→Solana routes are permissioned
- Need to register as MM with deBridge team
- deBridge Discord: https://discord.gg/debridge

## Solana filler
- Built: `/tmp/debridge_solana_filler.py` — ready to run once SOL + USDC funded
- Solana keypair generated: `66JBjBJMshXuvTX17iUuy9n9BZQksSRcQqC5K6UXFs97` (needs SOL + USDC/USDT)
- Program: `dst5MGcFPoBeREFAA5E3tU5ij8m5uVYwkzkSAbsLbNo`
