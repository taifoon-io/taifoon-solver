# Deployment Instructions

## 1. Create Private GitHub Repo

```bash
# Via GitHub CLI (if installed)
gh repo create yawningmonsoon/taifoon-solver --private --source=. --remote=origin --push

# Or manually:
# 1. Go to https://github.com/new
# 2. Owner: yawningmonsoon
# 3. Repo name: taifoon-solver
# 4. Visibility: Private
# 5. Create repository
```

## 2. Push to GitHub

```bash
cd ~/projects/taifoon-solver
git remote add origin git@github.com:yawningmonsoon/taifoon-solver.git
git branch -M main
git push -u origin main
```

## 3. Create GitHub Issues

Copy issues from `SOLVER_GITHUB_ISSUES_CORRECTED.md` in the spinner repo:
- Issue #1: SSE Client (✅ COMPLETE)
- Issue #2: Profit Calculator (TODO)
- Issue #3: Execution Engine (TODO)
- Issue #4: Integration Test (TODO)

## 4. Test Build

```bash
cd ~/projects/taifoon-solver
cargo build --release

# Should compile successfully to:
# target/release/taifoon-solver
```

## 5. Test Live Genome Stream

```bash
# Run solver (will connect to genome stream)
./target/release/taifoon-solver

# Expected output:
# 🚀 Taifoon Solver Starting...
# 📡 Genome SSE: https://api.taifoon.dev/api/genome/subscribe/sse
# 💰 Min Profit: $1
# 🔌 Connecting to genome stream...
# ✅ Connected to genome stream
# ⏳ Waiting for intents...
# 🎯 New intent detected: lifi_v2 (1 → 42161)
```

## 6. Next Steps

1. Implement profit calculator with real solver_intel.json fees
2. Test profit calculation accuracy
3. Implement executor for LiFi fills
4. Execute first mainnet fill
5. Track actual vs estimated profit

## Status

- ✅ Repo structure created
- ✅ Genome client implemented
- ✅ Compiles successfully
- ⏳ Waiting for GitHub repo creation
- ⏳ Waiting for profit calc implementation
