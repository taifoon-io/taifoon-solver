# B2B Solver Registration

How another protocol (e.g. **t3rn**) integrates the on-chain solver
registry exposed by the taifoon-solver stack.

This doc is for protocol engineers shipping a "register as a solver"
surface. The goal: a would-be solver clicks one button in your dApp,
gets recorded on-chain in a permissionless registry, and your backend
(or frontend) can read membership without trusting any off-chain
authority.

---

## 1. What the registry does

`SolverRegistryV1` is a permissionless, immutable Solidity contract.
Anyone can call `register()` from their EOA. The contract records
`msg.sender → block.timestamp` and emits `SolverRegistered`. Re-calling
is idempotent (returns silently, keeps the first-seen timestamp).

The contract has **no owner, no admin, no upgrade path, no fees, no
ban list**. Reputation, banning, and economic policy live off-chain
keyed by `address`.

### Why permissionless

Earlier registry designs assumed a Spinner service held an ECDSA key
that signed FillPermits and gated registration via signature
verification. In practice every B2B integrator wants their own UX,
their own off-chain reputation model, and their own allow/deny rules.
A permissionless on-chain bit-of-truth ("this address registered at
T") lets every integrator layer their own policy on top without
needing taifoon's signing infrastructure on the critical path.

If you need attested registrations (e.g. KYC-gated solvers), build the
attestation in your own contract and have it call
`SolverRegistryV1.register()` from your gatekeeper. The on-chain record
still ties to `msg.sender = your gatekeeper`, so downstream consumers
can distinguish self-registered solvers from your-attested solvers by
inspecting the caller.

---

## 2. Contract surface

```solidity
contract SolverRegistryV1 {
    mapping(address => uint256) public registeredAt;
    uint256 public solverCount;

    event SolverRegistered(address indexed solver, uint256 registeredAt);

    function register() external;
    function isRegistered(address solver) external view returns (bool);
}
```

`register()` is the only mutating function. Both reads are O(1) and
cheap to poll from a frontend.

### Deployments

| Chain            | chainId | Address                                       | Status   |
|------------------|---------|-----------------------------------------------|----------|
| Taifoon Devnet   | `36927` | `0x91b6a2abd429ef728270148f08eb04e4ec315f77`  | live     |

Mainnet addresses will be appended here when published.

---

## 3. Integration recipe — frontend

Minimal Next.js + wagmi flow (matches what t3rn's portal does at
[`portal.t3rn.io/solve/register`](https://portal.t3rn.io/solve/register)):

```ts
import { useAccount, useChainId, useSwitchChain, useWriteContract,
         useReadContract, useWaitForTransactionReceipt } from "wagmi";

const SOLVER_REGISTRY = {
  chainId: 36927,
  address: "0x91b6a2abd429ef728270148f08eb04e4ec315f77" as const,
} as const;

const ABI = [
  { type: "function", name: "register", stateMutability: "nonpayable",
    inputs: [], outputs: [] },
  { type: "function", name: "isRegistered", stateMutability: "view",
    inputs: [{ name: "solver", type: "address" }],
    outputs: [{ type: "bool" }] },
  { type: "function", name: "solverCount", stateMutability: "view",
    inputs: [], outputs: [{ type: "uint256" }] },
] as const;

export function RegisterButton() {
  const { address } = useAccount();
  const chainId     = useChainId();
  const { switchChain } = useSwitchChain();
  const { writeContractAsync } = useWriteContract();

  const isRegisteredQ = useReadContract({
    abi: ABI, address: SOLVER_REGISTRY.address,
    functionName: "isRegistered", args: address ? [address] : undefined,
    chainId: SOLVER_REGISTRY.chainId,
    query: { enabled: Boolean(address) },
  });

  const [txHash, setTxHash] = useState<`0x${string}` | null>(null);
  const receipt = useWaitForTransactionReceipt({
    hash: txHash ?? undefined,
    chainId: SOLVER_REGISTRY.chainId,
    query: { enabled: Boolean(txHash) },
  });

  if (chainId !== SOLVER_REGISTRY.chainId) {
    return <button onClick={() => switchChain({ chainId: SOLVER_REGISTRY.chainId })}>
      Switch to Taifoon devnet
    </button>;
  }
  if (isRegisteredQ.data) return <span>Already registered ✓</span>;
  if (receipt.isSuccess)  return <span>Registered — tx {txHash}</span>;

  return (
    <button onClick={async () => {
      const hash = await writeContractAsync({
        abi: ABI, address: SOLVER_REGISTRY.address,
        functionName: "register",
        chainId: SOLVER_REGISTRY.chainId,
      });
      setTxHash(hash);
    }}>Register on-chain</button>
  );
}
```

That's the entire integration. No API tokens, no CORS, no off-chain
key management — the user's wallet pays the gas and the registry is
the source of truth.

### Optional: also notify your off-chain backend

If you want a DB row (e.g. for reputation, email notifications, etc.),
have the user EIP-191-sign an off-chain message in parallel and POST
`{ address, signature, message, txHash }` to your own service.
**Treat the off-chain notification as best-effort** — on-chain is
canonical, so swallow network errors:

```ts
await fetch("/api/solver/register", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ address, signature, message, txHash }),
}).catch(() => {}); // on-chain registration succeeded; DB is optional
```

t3rn's portal does this — see `RegisterView.tsx` in
[`portal-2026`](https://github.com/t3rn/portal/blob/main/src/app/solve/register/RegisterView.tsx).

---

## 4. Integration recipe — backend

```ts
import { createPublicClient, http } from "viem";

const client = createPublicClient({
  transport: http("https://rpc.taifoon.dev"),
});

async function isSolverRegistered(address: `0x${string}`) {
  return client.readContract({
    abi: ABI,
    address: "0x91b6a2abd429ef728270148f08eb04e4ec315f77",
    functionName: "isRegistered",
    args: [address],
  });
}
```

Gate API token issuance / job assignment / reward payouts on
`isSolverRegistered(address) === true`. The read is one RPC call; cache
aggressively (the only state change is the boolean flipping from false
→ true, so a 60s cache is fine).

### Reading the event log

For historical registrations or to populate an audit ledger:

```ts
const logs = await client.getLogs({
  address: "0x91b6a2abd429ef728270148f08eb04e4ec315f77",
  event: {
    type: "event",
    name: "SolverRegistered",
    inputs: [
      { indexed: true, name: "solver", type: "address" },
      { indexed: false, name: "registeredAt", type: "uint256" },
    ],
  },
  fromBlock: 1_477_554n, // contract deploy block on Taifoon devnet
});
```

---

## 5. Layering reputation on top

The on-chain record is a single bit + timestamp. Reputation, fill
counts, payout history, and bans are off-chain concerns. Recommended
pattern:

```
+----------------------+        on-chain truth
|  SolverRegistryV1    |  ←──── registered ∈ {0,1}, registeredAt
+----------------------+
           ↑
           │  read on demand (cached)
           │
+----------------------+        off-chain policy
|  YourProtocolDB      |  ←──── reputation, banned, api_token,
|  e.g. postgres       |        payout_history, email, …
+----------------------+
```

Your off-chain DB row references the on-chain record by `address`. If
the on-chain registry hasn't seen the address, refuse the API token.
If your DB bans the address, refuse — but the on-chain bit is
unchanged (you can never burn someone else's on-chain registration).

This separation means each integrator runs their own policy. t3rn
might ban an address that taifoon still trusts and vice versa; the
on-chain bit is shared but the consequences are local.

---

## 6. Why this design

- **No custodial key**: no relayer to compromise, no admin to socially
  engineer.
- **No off-chain dependency in the critical path**: a solver can prove
  registration to a counterparty using `eth_call` on a public RPC.
- **Composable with attestation gateways**: if you need KYC-gated
  registration, deploy a gatekeeper contract that calls
  `register()` on behalf of the user after your checks pass.
- **Idempotent + replayable**: a buggy frontend that calls `register()`
  twice does not corrupt state and does not double-count.

If/when a more elaborate registry is needed (FillPermits, slashing,
bonded solvers, etc.), it will be a separate contract — V1 is
intentionally minimal so it never needs replacing for the
"yes/no, when" question.

---

## 7. Operational notes

- **Gas**: ~143k deploy, ~50k per `register()` on Taifoon devnet
  (effectively $0).
- **Event indexing**: the contract emits `SolverRegistered(solver,
  registeredAt)` exactly once per first registration. Idempotent calls
  do NOT emit a duplicate event. Indexers can use a uniqueness
  constraint on `solver` without de-dup logic.
- **Re-registration**: silent no-op. Off-chain consumers who care
  about "have they recently re-attested" should layer their own
  signed-message scheme on top.
- **Burning a registration**: not supported. Off-chain ban list
  overrides on-chain registration; the contract intentionally has no
  unregister path.

---

## 8. References

- Contract source: [`portal-2026/contracts/src/SolverRegistryV1.sol`](https://github.com/t3rn/portal/blob/main/contracts/src/SolverRegistryV1.sol)
- Forge tests: [`portal-2026/contracts/test/SolverRegistryV1.t.sol`](https://github.com/t3rn/portal/blob/main/contracts/test/SolverRegistryV1.t.sol)
- Deploy script: [`portal-2026/contracts/script/deploy-solver-registry-devnet.mjs`](https://github.com/t3rn/portal/blob/main/contracts/script/deploy-solver-registry-devnet.mjs)
- Deployments index: [`portal-2026/contracts/deployments/solver-registry-devnet.json`](https://github.com/t3rn/portal/blob/main/contracts/deployments/solver-registry-devnet.json)
- t3rn portal integration: [`portal-2026/src/app/solve/register/RegisterView.tsx`](https://github.com/t3rn/portal/blob/main/src/app/solve/register/RegisterView.tsx)
