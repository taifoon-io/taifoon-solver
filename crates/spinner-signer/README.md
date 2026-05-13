# spinner-signer

Scoped session-key signing for cross-chain Spinner operators.

## The pattern

A Spinner's master treasury (Safe on EVM, Squads multisig on Solana)
grants the running solver a **scoped session key** — a hot key the
binary holds, but which is restricted on-chain to only the calls the
solver actually needs to make. Spend caps, target allowlists, and
expiry are enforced by the scope contract on chain, not by the binary.

| Chain | Master | Scope contract | Session key |
|---|---|---|---|
| EVM | Safe (`safe-smart-account`) | Zodiac Roles Module | EOA |
| Solana | Squads V4 (`squads-protocol/v4`) | Squads spending-limit | ed25519 keypair |

Why this is materially safer than "ship the operator's raw EOA": the
worst-case blast radius if the session key leaks is the spend cap
remaining in the current period, not the entire Safe / multisig
balance. Rotating the key is a single transaction from the master
treasury, not an emergency migration.

## The trait + the two impls

```rust
pub trait SessionSigner: Send + Sync {
    type Transaction;
    type SignedTransaction;
    type TxHash;
    type Error;

    async fn wrap_for_session(&self, target_call: Self::Transaction)
        -> Result<Self::Transaction, Self::Error>;

    async fn dry_run_policy(&self, target_call: &Self::Transaction)
        -> Result<PolicyCheckResult, Self::Error>;

    async fn sign(&self, tx: Self::Transaction)
        -> Result<Self::SignedTransaction, Self::Error>;

    async fn broadcast(&self, signed: Self::SignedTransaction)
        -> Result<Self::TxHash, Self::Error>;
}
```

Two implementations:

- `evm::EvmSafeRolesSigner` — wraps the target call in
  `execTransactionWithRole` calldata against a Zodiac Roles Module
  installed on the Safe. Dry-runs use
  `execTransactionWithRoleReturnData(shouldRevert=false)` via an
  `eth_call`.
- `solana::SolanaSquadsSigner` — wraps the target call in a
  `spending_limit_use` Anchor instruction against the Squads V4
  program. Dry-runs use `simulateTransaction` with
  `replaceRecentBlockhash=true` so the signer doesn't need to fetch a
  blockhash before checking policy.

## Where to look upstream

- Safe Smart Account: <https://github.com/safe-global/safe-smart-account>
- Zodiac Roles Modifier: <https://github.com/gnosisguild/zodiac-modifier-roles>
- Squads V4 program + spending limits: <https://github.com/Squads-Protocol/v4>
- Squads spending-limits docs: <https://docs.squads.so/main/v4/spending-limits>

The constants `SQUADS_V4_PROGRAM_ID_B58` (in
`src/solana/squads_program.rs`) and the EVM Roles Module ABI (in
`src/evm/roles_abi.rs`) are direct quotations from upstream. Any
mismatch with the current upstream is a bug — file an issue.

## Setting up an operator

See `scripts/session-setup/` at the workspace root. The walkthrough:

```bash
# EVM track (Safe + Zodiac Roles)
./scripts/session-setup/evm/deploy-safe.sh
./scripts/session-setup/evm/install-roles-module.sh
ts-node ./scripts/session-setup/evm/configure-role.ts

# Solana track (Squads V4 spending limits)
./scripts/session-setup/solana/deploy-squads.sh
./scripts/session-setup/solana/configure-spending-limit.sh
```

After each track completes, copy the resulting addresses into
`config/session_config.json` and start `taifoon-solver` with
`SPINNER_EVM_SESSION_KEY=…` / `SPINNER_SOLANA_SESSION_KEY=…` set.

## Status

**Reference design. Not yet wired into `crates/executor/`.** The
executor's live fill path still uses the SelfHosted (Keychain) signer;
this crate is the proof that the SessionKey signing-mode is grounded
in real primitives and ready to wire in when the operator threat model
warrants the additional setup. The trait surface, ABI / Anchor
discriminators, and config wire format are stable; the executor
integration is a separate sprint.

The four test scenarios per chain
(`tests/evm_session_policy.rs`, `tests/solana_session_policy.rs`)
exercise the calldata-encoding and dry-run-decoding paths without
needing a live RPC. End-to-end coverage against a forked anvil (EVM)
or `solana-program-test` (Solana) is the next test-tier and is
documented at the top of each test file.
