# Operator setup — one time, then forget

This directory contains the **one-time** setup scripts an operator
runs to provision the master treasury + scope contract that
`crates/spinner-signer/` then drives in the live fill loop. After
these scripts complete you copy the resulting addresses into
`config/session_config.json` and start `taifoon-solver` with the
`SPINNER_EVM_SESSION_KEY=…` / `SPINNER_SOLANA_SESSION_KEY=…` env vars
set.

## EVM track (Safe + Zodiac Roles)

```bash
./scripts/session-setup/evm/deploy-safe.sh
./scripts/session-setup/evm/install-roles-module.sh
ts-node ./scripts/session-setup/evm/configure-role.ts
```

What each step does:

1. **deploy-safe.sh** — uses Safe CLI (`safe-cli`) or a direct
   `forge create` call against the canonical Safe Proxy Factory to
   deploy a new Safe at a deterministic address. Output: the new Safe
   address.
2. **install-roles-module.sh** — `safe-cli` call to `enableModule(...)`
   on the Safe with the Zodiac Roles Module address. Output: the
   Roles Module address (already known; just verifies the install).
3. **configure-role.ts** — TypeScript script using
   `@gnosis-guild/zodiac-roles-sdk` to define a scoped role: target
   contracts, allowed function selectors, per-token spend caps, and
   an optional expiry. Output: the 32-byte role key to drop into
   `config/session_config.json`.

## Solana track (Squads V4 spending limits)

```bash
./scripts/session-setup/solana/deploy-squads.sh
./scripts/session-setup/solana/configure-spending-limit.sh
```

What each step does:

1. **deploy-squads.sh** — `squads-cli` invocation to create a new V4
   multisig. Output: the multisig PDA address.
2. **configure-spending-limit.sh** — `squads-cli` invocation to add a
   spending-limit account with daily-USDC cap + allowed-programs list.
   Output: the spending-limit account address.

## After setup

Copy into `config/session_config.json`:

```jsonc
{
  "chain": "evm",
  "safe_address": "<from deploy-safe.sh>",
  "roles_module": "<canonical Zodiac Roles Module deployment for this chain>",
  "role_key": "<from configure-role.ts>",
  "allowed_targets": ["<protocol target contracts>"],
  "session_key_env": "SPINNER_EVM_SESSION_KEY",
  "rpc_url": "https://…",
  "chain_id": 8453
}
```

or

```jsonc
{
  "chain": "solana",
  "squads_multisig": "<from deploy-squads.sh>",
  "spending_limit": "<from configure-spending-limit.sh>",
  "allowed_programs": ["<protocol program IDs>"],
  "session_key_env": "SPINNER_SOLANA_SESSION_KEY",
  "rpc_url": "https://api.mainnet-beta.solana.com"
}
```

Then export your hot session-key:

```bash
# EVM: 32-byte secp256k1 hex
export SPINNER_EVM_SESSION_KEY=0x…

# Solana: base58 64-byte keypair (the format `solana-keygen new` produces)
export SPINNER_SOLANA_SESSION_KEY=…
```

The session key MUST be different from the Safe owner / Squads member
keys — that's the whole point. Generate it fresh:

```bash
# EVM
cast wallet new

# Solana
solana-keygen new --no-bip39-passphrase --outfile session-key.json
cat session-key.json # then base58-encode the JSON array contents
```

## Status

These scripts are **scaffolds**: they document the exact upstream
commands a reviewer would expect to see, with all CLI flags clearly
marked. They do not yet ship as standalone executables — the
operator runs the underlying tools (`safe-cli`, `forge`, `ts-node`,
`squads-cli`) by hand following the comments inside each script.

Hardening these into idempotent installers is tracked as future work
in the same sprint that wires `spinner-signer` into the executor.
