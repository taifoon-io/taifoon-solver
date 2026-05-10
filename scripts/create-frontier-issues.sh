#!/usr/bin/env bash
# create-frontier-issues.sh
#
# Bulk-create the 14 Frontier execution issues against
# yawningmonsoon/taifoon-solver. Reads the issue series from
# ../FRONTIER_GITHUB_ISSUES.md (sibling to this script's parent dir).
#
# Idempotent: skips any issue whose title already exists in the open list.
# Requires: gh CLI authenticated against the target repo.
#
# Usage:
#   ./scripts/create-frontier-issues.sh           # post all issues
#   ./scripts/create-frontier-issues.sh --dry     # preview without posting
set -euo pipefail

REPO="yawningmonsoon/taifoon-solver"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE="${SCRIPT_DIR}/../../FRONTIER_GITHUB_ISSUES.md"   # spinner/FRONTIER_GITHUB_ISSUES.md
DRY=0

[[ "${1:-}" == "--dry" ]] && DRY=1

if ! command -v gh >/dev/null 2>&1; then
  echo "ERROR: gh CLI not installed. Install: https://cli.github.com/" >&2
  exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
  echo "ERROR: gh CLI not authenticated. Run: gh auth login" >&2
  exit 1
fi
if [[ ! -f "$SOURCE" ]]; then
  echo "ERROR: source file not found: $SOURCE" >&2
  exit 1
fi

echo "▸ Source: $SOURCE"
echo "▸ Target: $REPO"
echo "▸ Mode:   $([[ $DRY == 1 ]] && echo DRY-RUN || echo POST)"

# Pull existing open + closed titles so we don't duplicate.
existing="$(gh issue list --repo "$REPO" --state all --limit 1000 --json title --jq '.[].title')"

# Define the 14 issues directly here so the script is self-contained even if
# the source file is missing. Keep in sync with FRONTIER_GITHUB_ISSUES.md.
ISSUES=$(cat <<'JSON'
[
  {
    "title": "Phase A · TSUL LICENSE.md at solver repo root",
    "body": "Frontier judges look at the repo's LICENSE before they look at code. Without an operative LICENSE.md the wedge ('fair-code, on-chain donut') is unsubstantiated.\n\n**Acceptance criteria:**\n- LICENSE.md at repo root contains the four TSUL rules (n8n's three + the donut clause)\n- Worked examples include at least one Solana-flavored ALLOWED and one NOT ALLOWED case\n- taifooon@proton.me mailto present\n- README points to the canonical LICENSE.md\n\n**Blocks:** #2, #3, #7",
    "labels": "frontier:hackathon,license:tsul,fair-code,phase:A-legal,priority:P0",
    "milestone": "frontier-A-legal"
  },
  {
    "title": "Phase A · CONTRIBUTOR_LICENSE_AGREEMENT.md + CLA bot",
    "body": "TSUL is only enforceable if contributions are clearly licensed in. n8n's CLA pattern is the simplest path to relicensing rights.\n\n**Acceptance criteria:**\n- CLA mirrors n8n's structure, six clauses, plain English\n- On-chain signing via BuildersRegistry.submitAdapter() documented\n- CLA_VERSION_HASH constant added to BuildersRegistry.sol\n- .github/cla-bot.yml for off-chain PRs\n- taifooon@proton.me on contact section\n\n**Blocked by:** #1",
    "labels": "frontier:hackathon,license:tsul,fair-code,phase:A-legal,priority:P1",
    "milestone": "frontier-A-legal"
  },
  {
    "title": "Phase B · Flywheel-economy framing replaces 'bounty/prize' everywhere",
    "body": "The pitch is 'co-own a route under TSUL', not 'win a prize'. Reviewers from Drift / Altitude / Ellipsis will spot any framing inconsistency.\n\n**Acceptance criteria:**\n- /builders/bounties header reads 'Open Routes'\n- No card quotes a USD or token amount as a 'reward'\n- bounties.json: zero residual upfrontUsdc/upfrontFoon/prize/reward_* fields\n- /os/submit-job uses 'submit a route to the flywheel' framing\n- Every TSUL sash links to /legal/tsul\n- /builders landing has the flywheel diagram\n\n**Blocks:** #4, #6",
    "labels": "frontier:hackathon,frontier:wedge,phase:B-flywheel,priority:P0",
    "milestone": "frontier-B-flywheel"
  },
  {
    "title": "Phase B · bounties.xml data layer purge — remove every FOON/USDC/upfront field",
    "body": "Surface text is one thing; the data layer is another.\n\n**Acceptance criteria:**\n- bounties.xml schema validation: zero <upfront_*> elements\n- scripts/sync-bounties.mjs rejects deprecated fields\n- open-mamba/README.md no longer shows 'XK FOON' stake column\n- open-mamba/reviewers.xml: <stake_foon> renamed or removed\n- vendors.json audited\n\n**Blocked by:** #3",
    "labels": "frontier:hackathon,license:tsul,phase:B-flywheel,priority:P0",
    "milestone": "frontier-B-flywheel"
  },
  {
    "title": "Phase C · Apply taifoon-design-system zip — webfonts + tokens unified",
    "body": "JetBrains Mono Variable + Inter Variable must load consistently.\n\n**Acceptance criteria:**\n- public/fonts/JetBrainsMono-Variable.woff2 + Inter-Variable.woff2 shipped\n- @font-face declarations in globals.css\n- --font-mono resolves to 'JetBrains Mono', ui-monospace, ...\n- Zero matches for divergent stacks anywhere in src/\n- Visual smoke test passes\n\n**Blocks:** #6",
    "labels": "frontier:hackathon,phase:C-design,priority:P0",
    "milestone": "frontier-C-design"
  },
  {
    "title": "Phase C · Page-by-page design audit — HeroShell, geo-* signature, .cell CTAs",
    "body": "Design system has shared atoms; pages not using them feel orphan.\n\n**Acceptance criteria:**\n- Every /builders/* page uses HeroShell at top\n- Every page has coordinate-grid backdrop\n- Primary CTAs use .cell or .tf-cta-blue\n- [SECTION TITLE] labels use MONO 12 0.20em blueLabel\n- /legal/tsul, /os/dispatch, /os/submit-job audited\n- Zero fontWeight: 900 on user-facing text\n\n**Blocked by:** #5",
    "labels": "frontier:hackathon,phase:C-design,priority:P1",
    "milestone": "frontier-C-design"
  },
  {
    "title": "Phase D · taifoon-solver README sells the wedge",
    "body": "The README is the first thing every Frontier judge reads.\n\n**Acceptance criteria:**\n- Headline: 'The first solver-as-an-OS, fair-code, on-chain donut economics. Solana-native cross-chain fills with cryptographic settlement proofs.'\n- Four wedge bullets verbatim\n- Live demo URL: solver.taifoon.dev\n- Architecture diagram\n- LICENSE (Apache 2.0) at root\n- Cross-link parity to taifoon.io/legal/tsul, /builders/bounties, /os/dispatch\n\n**Blocked by:** #1",
    "labels": "frontier:hackathon,frontier:wedge,license:apache-2.0,phase:D-solver,priority:P0",
    "milestone": "frontier-D-solver"
  },
  {
    "title": "Phase D · FRONTIER_DEMO.md — 90-second flow scripted second-by-second",
    "body": "Demo days punish ad-libbing.\n\n**Acceptance criteria:**\n- FRONTIER_DEMO.md at repo root with second-by-second cues\n- Cue list covers /os/dispatch → /os/submit-job → /builders/bounties → /legal/tsul → live Solana fill → V5 proof\n- Backup paths documented\n- All URLs exist before recording\n\n**Blocked by:** #7",
    "labels": "frontier:hackathon,frontier:wedge,phase:D-solver,priority:P0",
    "milestone": "frontier-D-solver"
  },
  {
    "title": "Phase D · Solana-Mayan adapter scaffold — live demo path",
    "body": "Demo MUST execute a real Solana fill on stage.\n\n**Acceptance criteria:**\n- MayanSolanaAdapter.sol deployed on devnet 36927\n- Python decoder under templates/adapter-v1/mayan_solana/\n- pytest tests/test_mayan_solana.py passes\n- Live RPC handle in .env.solver\n- Demo-day fallback: pre-recorded tx + V5 proof JSON in demo/fallback/\n\n**Blocked by:** #1, #7",
    "labels": "frontier:hackathon,license:tsul,phase:D-solver,priority:P0",
    "milestone": "frontier-D-solver"
  },
  {
    "title": "Phase D+ · Wire /os/dispatch to LIVE open-mamba reviewer fleet",
    "body": "THE WINNING MOVE. Every other Frontier solver demos a swap. Ours demos autonomous work happening live, on screen.\n\n**Acceptance criteria:**\n- /os/dispatch 'Reviews in flight' panel pulls from agent.taifoon.dev/reviews/active\n- Each active review pulses Volt while running\n- Verdict transitions animate with cubic-bezier(0.16, 1, 0.3, 1)\n- Brain indicator pulses on every reviewer-agent heartbeat\n- WebSocket fallback if endpoint down\n- Reviewers under open-mamba/agents/* discoverable\n\n**Blocked by:** #11",
    "labels": "frontier:hackathon,frontier:wedge,phase:D+-brain,priority:P0",
    "milestone": "frontier-D+-brain"
  },
  {
    "title": "Phase D+ · Live taifoon-mamba dispatcher — autonomous job triage with on-screen heartbeat",
    "body": "When the demo presenter clicks 'Submit a job' on stage, the on-screen indicator MUST show the OS triaging in real time.\n\n**Acceptance criteria:**\n- taifoon-mamba dispatcher running at agent.taifoon.dev/mamba/triage\n- /os/submit-job receipt panel polls /api/dispatch/triage-status every 2s\n- Status transitions: queued → reading → classifying → drafting → posted\n- On 'posted', receipt links to new bounty\n- Triage SLA: 72h prod, ≤5s demo\n- Heartbeat indicator alive\n\n**Blocks:** #10",
    "labels": "frontier:hackathon,frontier:wedge,phase:D+-brain,priority:P0",
    "milestone": "frontier-D+-brain"
  },
  {
    "title": "Phase E · 3-slide pitch deck (PDF) — load-bearing 60 seconds",
    "body": "Colosseum's submission form takes a deck. We get 3 slides max attention.\n\n**Acceptance criteria:**\n- Slide 1: wedge headline + four bullets\n- Slide 2: live /os/dispatch screenshot annotated\n- Slide 3: V5-proof flow + license posture (Apache 2.0 + TSUL)\n- Brand-compliant: black background, JetBrains Mono, no emojis\n- Exported as PDF, ≤1MB\n\n**Blocked by:** #7",
    "labels": "frontier:hackathon,phase:E-demo,priority:P1",
    "milestone": "frontier-E-demo"
  },
  {
    "title": "Phase E · Demo recording + Frontier submission",
    "body": "The actual submission to Colosseum. Cannot be auto-shipped.\n\n**Acceptance criteria:**\n- 90-second screen recording follows FRONTIER_DEMO.md cues\n- Uploaded unlisted to YouTube\n- Submission form filled: project name, one-liner, long description, GitHub URL, demo URL, deck PDF, video URL, license posture\n- Submitted before 23:59 May 11, 2026 UTC\n- Confirmation email screenshot saved\n\n**Blocked by:** #8, #9, #10, #11, #12",
    "labels": "frontier:hackathon,phase:E-demo,priority:P0",
    "milestone": "frontier-E-demo"
  },
  {
    "title": "Phase F · Post-submission compounding — keep the OS running",
    "body": "After May 11, the win is sustained traffic.\n\n**Acceptance criteria:**\n- Weekly scheduled task frontier-weekly-digest\n- At least one new contributor lands a merge within 2 weeks\n- /os/dispatch shows non-zero activity 50%+ of business hours during judging window\n- Follow-up blog post on demo outcomes + accelerator status",
    "labels": "phase:F-compound,priority:P2",
    "milestone": "frontier-F-compound"
  }
]
JSON
)

# Iterate
echo "$ISSUES" | python3 -c "
import json, sys
issues = json.load(sys.stdin)
for i in issues:
    print('---ISSUE---')
    print('TITLE:', i['title'])
    print('LABELS:', i['labels'])
    print('MILESTONE:', i['milestone'])
    print('BODY:')
    print(i['body'])
" | while IFS= read -r line; do
  if [[ "$line" == "---ISSUE---" ]]; then
    [[ -n "${current_title:-}" ]] && post_issue
    current_title=""; current_labels=""; current_milestone=""; current_body=""
    in_body=0
  elif [[ "$line" == TITLE:* ]]; then
    current_title="${line#TITLE: }"
  elif [[ "$line" == LABELS:* ]]; then
    current_labels="${line#LABELS: }"
  elif [[ "$line" == MILESTONE:* ]]; then
    current_milestone="${line#MILESTONE: }"
  elif [[ "$line" == BODY: ]]; then
    in_body=1
  elif [[ $in_body == 1 ]]; then
    current_body+="${line}"$'\n'
  fi
done
[[ -n "${current_title:-}" ]] && post_issue

post_issue() {
  if echo "$existing" | grep -Fxq "$current_title"; then
    echo "  skip (exists): $current_title"
    return
  fi
  if [[ $DRY == 1 ]]; then
    echo "  [dry] would post: $current_title  [labels: $current_labels  milestone: $current_milestone]"
    return
  fi
  gh issue create --repo "$REPO" \
    --title "$current_title" \
    --body "$current_body" \
    --label "$current_labels" \
    --milestone "$current_milestone" 2>&1 | tail -1
  sleep 1
}

echo "▸ done."
