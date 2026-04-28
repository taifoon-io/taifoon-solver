# Mamba Tracking — hackathon-colosseum

**Date:** 2026-04-28
**Mamba host:** `http://localhost:1337` (PID 2970, listening since current session)
**Project tag:** `hackathon-colosseum`

## Daily-summary cron

Registered via `POST /api/schedules`:

| field            | value                                                                                                                                |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `id`             | `col-daily-summary`                                                                                                                  |
| `cron_expr`      | `0 9 * * *` (09:00 UTC daily)                                                                                                        |
| `project`        | `hackathon-colosseum`                                                                                                                |
| `assigned_agent` | `overseer`                                                                                                                           |
| `model`          | `nemotron/taifoon` (server default)                                                                                                  |
| `priority`       | 6                                                                                                                                    |
| `enabled`        | `true`                                                                                                                               |
| `next_run_at`    | `2026-04-28T09:00:00Z`                                                                                                               |
| `payload`        | "Daily summary for hackathon-colosseum: enumerate completed and in-flight tasks since the previous 09:00, surface blockers, and post a one-paragraph status digest." |

Confirmed by `GET /api/schedules` — entry present with `fire_count: 0` and `next_run_at` set.

To delete or replace: `DELETE /api/schedules/col-daily-summary` (re-POST with the same `id` upserts).

## Tasks endpoint — hackathon-colosseum jobs

`GET /api/tasks?project=hackathon-colosseum` returns 8 tasks (3 running, 5 done) — all dispatched today between 08:33 and 08:35 UTC. Sample IDs:

```
e7cd619c  running  coder  (this task — col-p6-dashboard)
794ddd25  done     coder  col-p1b k8s sidecar manifests
a3ad4630  running  coder  col-p5 spinner sandbox doc
53c1d391  done     coder
da5363c1  running  coder
b0eff06b  done     coder
4df5542b  done     coder
8d637bbb  done     coder
```

Endpoint contract verified: filters by `project` querystring; returns full task records including `payload`, `assigned_agent`, `status`, `cost_usd`, `tokens_in/out`, timestamps. Useful for the overseer's daily summary.

## Web dashboard

- **URL:** `http://localhost:1337/` — served from `crates/mamba-api/static/index.html` (~73 KB; Inter + JetBrains Mono fonts; mint-on-near-black).
- **API surface used by the dashboard:**
  - `GET /api/auth/status`
  - `GET /api/tasks?limit=…`
  - `GET /api/analytics/global` / `/daily` / `/agents` / `/projects`
  - `GET /api/nemotron/health` (+ `/api/nemotron/*` for inference UI)
- **Schedules + project-filtered task views** are NOT yet wired into the dashboard's analytics tabs — those panels currently consume `/api/analytics/*` aggregates only. The col-daily-summary cron will fire and produce a task record, which will appear in the standard tasks list, but the dashboard does not yet have a dedicated "schedules" panel. Filing that as out-of-scope for this task.

## Operational notes

- `next_run_at` is computed server-side from the cron expression at create time — confirmed via `crates/mamba-api/src/triggers.rs:215-246`.
- Schedules persist to DuckDB (same store as tasks); a mamba restart will not lose the registration.
- Project filter on `/api/tasks` was confirmed working with 200 OK + 7.2 KB body.
