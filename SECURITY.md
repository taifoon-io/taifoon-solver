# Security Policy

## Supported Versions

Only the latest commit on `master` receives security fixes. Older tags or forks
are not supported.

| Version / Branch | Supported |
|------------------|-----------|
| `master` (latest) | Yes |
| Any other branch / tag | No |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Email **hello@taifoon.dev** with the subject line `[SECURITY] <brief description>`.

We aim to acknowledge every report within **48 hours** and provide a fix or
mitigation timeline within 7 days. If you do not hear back within 48 hours,
send a follow-up reply to the same thread — reports occasionally land in spam.

Include in your report:
- Affected component (file and line number if known)
- Description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept (if applicable)
- Whether you believe the issue is exploitable in the current production deployment

We will credit reporters in the release notes unless you prefer to remain
anonymous.

## What NOT to Report

The following are known, intentional design choices and are out of scope:

| Observation | Reason it is intentional |
|-------------|--------------------------|
| Rate limiting is not enforced on `/api/solver/stream` | The SSE endpoint streams fill events; enforcing rate limiting there would drop legitimate dashboard connections. |
| `Cache-Control: no-cache` and related headers absent on `/api/solver/stream` | SSE requires the browser to hold the connection open. Caching headers that close or buffer the response break the live feed by design. |
| `SOLVER_PRIVATE_KEY` accepted via environment variable | The env-var path is the documented fallback for CI and ephemeral environments. See key management note below. |
| `/health` endpoint requires no authentication | Intentional — monitoring and load balancers must probe liveness without credentials. |

## Key Management

The solver **never stores private keys on disk**. Keys flow through the system
as follows:

- **macOS (recommended):** Keys are read directly from the macOS Keychain via
  the `security` CLI at startup. The raw key string exists in memory for less
  than one microsecond before being parsed into a signer object and explicitly
  zeroed. It is never written to disk, logged, or transmitted.

- **Linux / CI:** Keys are injected into the process environment at runtime via
  a secrets manager (HashiCorp Vault, AWS Secrets Manager, GitHub Actions
  secrets). They are never written to files on disk.

- **Environment variable fallback:** Acceptable for ephemeral CI runs. Not
  acceptable for persistent deployments — see `SECURITY_ONBOARDING.md` §2.3
  for the specific risks.

If you discover a code path that writes, logs, or transmits a private key —
even transiently — that is a critical vulnerability. Please report it
immediately via the email above.

## Scope

In-scope for security reports:
- Authentication / authorization bypasses on any `/api/solver/*` route
- Private key or secret exfiltration via any path (logs, error messages, HTTP
  responses, disk writes)
- Remote code execution via intent processing or RPC response parsing
- SQLite injection in the outcome or wallet databases
- Supply-chain vulnerabilities in direct Cargo or npm dependencies

Out of scope:
- Rate limiting (see above)
- Missing HTTP security headers on SSE endpoints (see above)
- Vulnerabilities in Solana / EVM node software or bridge protocols themselves
- Issues requiring physical access to the operator's machine
