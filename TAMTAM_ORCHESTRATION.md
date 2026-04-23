# TamTam Orchestration Guide for Taifoon Solver

**Goal**: Use TamTam (3h4x/tamtam) to automate the complete delivery of the taifoon-solver dashboard and executor.

## What is TamTam?

TamTam is an agent management dashboard for Claude CLI. It allows you to:
- Define reusable instruction blocks (skills)
- Compose skills into agents
- Run agents on demand or on schedule
- Watch output stream in real-time
- Manage multiple projects from one dashboard

**Location**: `/Users/mbultra/projects/tamtam`
**Dashboard**: http://localhost:1337 (when running)

## Skills Created

I've created two comprehensive skills in `/Users/mbultra/projects/tamtam/data/skills/taifoon-solver/`:

### 1. `build-dashboard.md`
Builds the Next.js 15 dashboard frontend with:
- SSE integration for real-time updates
- 1-page design matching BRAND.md
- Components: IntentsStream, PerformanceStats, etc.
- Complete implementation code

### 2. `build-executor.md`
Implements the on-chain executor with:
- LiFi protocol fill execution
- Safety features (simulation mode, balance checks)
- Integration with solver-main
- Testnet testing strategy

## How to Use TamTam to Deliver

### Option A: Manual Execution (Recommended for First Time)

**Step 1**: Start TamTam
```bash
cd /Users/mbultra/projects/tamtam
pnpm dev
```

**Step 2**: Open Dashboard
Navigate to http://localhost:1337

**Step 3**: Configure Workspace
- Go to Settings
- Set workspace path to `/Users/mbultra/projects`
- TamTam will auto-discover taifoon-solver project

**Step 4**: Create Agent for Dashboard
1. Go to `/agents`
2. Click "New Agent"
3. Fill in:
   - **Name**: `Taifoon Solver Dashboard Builder`
   - **Description**: `Build Next.js dashboard with SSE integration`
   - **Project**: `taifoon-solver`
   - **Model**: `sonnet` (claude-sonnet-4.5)
   - **Prompt**:
     ```
     Build the complete Next.js 15 dashboard for the taifoon-solver project.

     Follow the instructions in the "Build Taifoon Solver Dashboard" skill exactly.

     The solver API is already running on port 8082 with SSE endpoints.
     Your job is to create the frontend that consumes this API.

     Read BRAND.md and DELIVERY_PLAN.md for design specs.

     When complete, verify:
     - Dashboard loads at http://localhost:3000
     - SSE connection shows "LIVE" status
     - Intents appear in real-time
     - All components render correctly
     ```
   - **Skills**: Select `Build Taifoon Solver Dashboard` (from taifoon-solver category)
4. Click "Create Agent"

**Step 5**: Run Dashboard Agent
1. Click "Run" on the Dashboard Builder agent
2. Watch real-time output as Claude builds the dashboard
3. Claude will:
   - Create Next.js app structure
   - Implement all components
   - Configure Tailwind CSS
   - Install dependencies
   - Test the build

**Step 6**: Create Agent for Executor
1. Go to `/agents`
2. Click "New Agent"
3. Fill in:
   - **Name**: `Taifoon Solver Executor Builder`
   - **Description**: `Implement on-chain fill execution (LiFi first)`
   - **Project**: `taifoon-solver`
   - **Model**: `sonnet`
   - **Prompt**:
     ```
     Implement the executor module for on-chain fill execution.

     Follow the instructions in the "Build Taifoon Solver Executor" skill exactly.

     Start with LiFi protocol and implement:
     - Basic executor structure
     - Safety features (SIMULATION_MODE, balance checks)
     - Integration with solver-main
     - Placeholder for real execution

     IMPORTANT: Keep SIMULATION_MODE=true by default.

     When complete, verify:
     - cargo build --release succeeds
     - Executor wired into solver-main
     - Events emitted on fill attempts
     - Logs show simulation mode
     ```
   - **Skills**: Select `Build Taifoon Solver Executor`
4. Click "Create Agent"

**Step 7**: Run Executor Agent
1. Click "Run" on the Executor Builder agent
2. Watch as Claude implements the executor
3. Verify compilation succeeds

### Option B: Scheduled Execution (For Ongoing Maintenance)

You can schedule these agents to run automatically:

**Dashboard Agent**:
- Schedule: `0 9 * * *` (daily at 9am)
- Use case: Keep dashboard up to date with latest design changes

**Executor Agent**:
- Schedule: `0 10 * * 1` (weekly on Monday at 10am)
- Use case: Regular security audits and updates

## Verification Checklist

After TamTam completes both agents:

**Dashboard** (`/Users/mbultra/projects/taifoon-solver/dashboard/`):
- [ ] Directory exists
- [ ] `package.json` created
- [ ] All components in `components/` directory
- [ ] SSE hook in `hooks/` directory
- [ ] `npm run dev` starts successfully
- [ ] Dashboard loads at http://localhost:3000
- [ ] Connects to API at http://localhost:8082

**Executor** (`/Users/mbultra/projects/taifoon-solver/crates/executor/`):
- [ ] `src/lib.rs` implemented
- [ ] `Cargo.toml` updated with dependencies
- [ ] `cargo build --release` succeeds
- [ ] SIMULATION_MODE enabled by default
- [ ] Safety features present

**Integration**:
- [ ] Executor wired into `crates/solver-main/src/main.rs`
- [ ] Events emitted for IntentSolved
- [ ] Logs show execution attempts

## Monitoring Execution

TamTam provides real-time monitoring:

1. **Terminal View**: See token-by-token output as Claude works
2. **Run History**: All past runs with timestamps and status
3. **Logs**: Detailed logs of every action taken
4. **Notifications**: Get alerts when runs complete (configure webhooks)

## Advanced: Creating a Complete Pipeline

You can chain agents together for a full release pipeline:

**Agent 1**: Build Dashboard
**Agent 2**: Build Executor
**Agent 3**: Run Tests (`cargo test && cd dashboard && npm test`)
**Agent 4**: Commit Changes (`git add -A && git commit -m "..."`)
**Agent 5**: Push to GitHub (`git push origin master`)

TamTam's release pipeline feature can orchestrate this automatically with quality gates.

## Troubleshooting

**Skills Not Showing Up**:
- Restart TamTam: `pnpm restart`
- Skills are auto-scanned from `data/skills/` on startup

**Agent Fails**:
- Check TamTam logs: `pnpm logs`
- Re-run the agent with more context
- Adjust the prompt to be more specific

**Project Not Found**:
- Verify workspace path in Settings
- Ensure taifoon-solver is a git repo
- Refresh projects list in TamTam

## Benefits of Using TamTam

1. **Automated Execution**: No manual copy-paste of code
2. **Real-time Monitoring**: Watch Claude work in real-time
3. **Reproducible**: Skills are versioned, agents can be re-run
4. **Scheduled**: Run nightly builds, weekly audits automatically
5. **Multi-project**: Manage solver + spinner + other projects from one dashboard
6. **CI Integration**: Trigger agents on GitHub webhook events

## Next Steps

After TamTam completes the build:

1. **Test Dashboard**:
   ```bash
   cd /Users/mbultra/projects/taifoon-solver/dashboard
   npm run dev
   ```
   Visit http://localhost:3000

2. **Test Solver + Dashboard Together**:
   ```bash
   # Terminal 1: Run solver
   cd /Users/mbultra/projects/taifoon-solver
   cargo run --release

   # Terminal 2: Run dashboard
   cd dashboard
   npm run dev
   ```

3. **Commit Results**:
   ```bash
   cd /Users/mbultra/projects/taifoon-solver
   git add -A
   git commit -m "feat(dashboard+executor): complete Phase 3 & 4 delivery via TamTam

   - Dashboard: Next.js 15 with SSE integration
   - Executor: LiFi fill execution (simulation mode)
   - Orchestrated via TamTam skills and agents"
   git push origin master
   ```

---

**TamTam makes the complete solver delivery automated, monitored, and reproducible.**

Skills location: `/Users/mbultra/projects/tamtam/data/skills/taifoon-solver/`
Dashboard: http://localhost:1337
