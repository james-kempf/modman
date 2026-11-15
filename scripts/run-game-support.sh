#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Local game-support runner. It picks one pending game, lets the `claude` CLI
# (headless, on your claude.ai subscription login, never the API) research it
# and complete its definition in an isolated git worktree, checks the result,
# then commits, pushes vessel/game-support-<domain> and opens a PR.
#
# Your main checkout is never modified: all work happens in a worktree
# created from origin/<base> under the cache directory.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/run-game-support.sh [options]

  --game <domain>   Work on this Nexus domain instead of the next pending game.
  --base <branch>   Base branch to start from and open the PR against
                    (default: $VESSEL_GAME_SUPPORT_BASE or develop).
  --dry-run         Pick, research and run all checks, but do not commit,
                    push or open a PR. The worktree is kept for review.
  --pick-only       Only fetch and print which game would be picked.
  --timeout <dur>   Time limit for the claude run (timeout(1) syntax,
                    default 2h).
  -h, --help        Show this help.

Environment:
  VESSEL_GAME_SUPPORT_BASE   Default base branch (develop).
  VESSEL_GAME_SUPPORT_CACHE  Worktrees, logs and cargo target dir
                             (default: ${XDG_CACHE_HOME:-~/.cache}/vessel-game-support).
EOF
}

die() { echo "error: $*" >&2; exit 1; }
log() { echo "[game-support] $*" >&2; }

GAME=""
BASE="${VESSEL_GAME_SUPPORT_BASE:-develop}"
DRY_RUN=0
PICK_ONLY=0
CLAUDE_TIMEOUT="2h"

while [ $# -gt 0 ]; do
  case "$1" in
    --game) [ $# -ge 2 ] || die "--game needs a value"; GAME="$2"; shift 2 ;;
    --game=*) GAME="${1#*=}"; shift ;;
    --base) [ $# -ge 2 ] || die "--base needs a value"; BASE="$2"; shift 2 ;;
    --base=*) BASE="${1#*=}"; shift ;;
    --timeout) [ $# -ge 2 ] || die "--timeout needs a value"; CLAUDE_TIMEOUT="$2"; shift 2 ;;
    --timeout=*) CLAUDE_TIMEOUT="${1#*=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --pick-only) PICK_ONLY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done
[[ "$GAME" =~ ^[a-z0-9]*$ ]] || die "--game must be a Nexus domain ([a-z0-9]+)"
[[ "$BASE" =~ ^[A-Za-z0-9._/-]+$ ]] || die "invalid --base '$BASE'"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
PICKER="$SCRIPT_DIR/pick_game_support_target.py"
CHECKER="$SCRIPT_DIR/check_game_support_note.py"
PROMPT_FILE="$REPO/.github/prompts/add-game-support.md"
CACHE="${VESSEL_GAME_SUPPORT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/vessel-game-support}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"

# Never spend API credits: drop every variable that would make the CLI use
# an API key, a custom endpoint or a cloud provider instead of the
# claude.ai subscription login.
unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN ANTHROPIC_BASE_URL \
  ANTHROPIC_CUSTOM_HEADERS ANTHROPIC_BEDROCK_BASE_URL ANTHROPIC_VERTEX_BASE_URL \
  ANTHROPIC_VERTEX_PROJECT_ID ANTHROPIC_FOUNDRY_API_KEY ANTHROPIC_FOUNDRY_BASE_URL \
  ANTHROPIC_FOUNDRY_RESOURCE CLAUDE_CODE_USE_BEDROCK CLAUDE_CODE_USE_VERTEX \
  CLAUDE_CODE_USE_FOUNDRY CLAUDE_CODE_OAUTH_TOKEN CLAUDE_CODE_API_KEY_HELPER_TTL_MS \
  AWS_BEARER_TOKEN_BEDROCK
# Running from inside another Claude Code session must not leak its state.
unset CLAUDECODE CLAUDE_CODE_ENTRYPOINT CLAUDE_CODE_SESSION_ID

# ---- preflight -----------------------------------------------------------
for tool in git python3 cargo flock timeout; do
  command -v "$tool" >/dev/null || die "'$tool' not found in PATH"
done
{ [ -f "$PICKER" ] && [ -f "$CHECKER" ]; } || die "picker/checker missing next to $0"
[ -f "$PROMPT_FILE" ] || die "prompt file missing: $PROMPT_FILE"

if [ "$PICK_ONLY" -eq 0 ]; then
  command -v claude >/dev/null || die "'claude' CLI not found; install Claude Code"
  auth="$(claude auth status --json 2>/dev/null)" || die "'claude auth status' failed; run 'claude auth login'"
  python3 -I - "$auth" <<'PY' || die "claude must be logged in with a claude.ai subscription (run 'claude auth login'); API keys and cloud providers are refused"
import json, sys
a = json.loads(sys.argv[1])
ok = a.get("loggedIn") is True and a.get("authMethod") == "claude.ai" \
    and a.get("apiProvider") == "firstParty"
sys.exit(0 if ok else 1)
PY
  # Settings files could re-inject an API key or key helper.
  python3 -I - "$HOME/.claude/settings.json" "$HOME/.claude/settings.local.json" \
    "/etc/claude-code/managed-settings.json" <<'PY' || die "a Claude settings file sets an API key or apiKeyHelper; remove it so the subscription login is used"
import json, sys, os
bad = {"ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_USE_BEDROCK",
       "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY", "ANTHROPIC_BASE_URL"}
for p in sys.argv[1:]:
    if not os.path.isfile(p):
        continue
    d = json.load(open(p))
    if d.get("apiKeyHelper") or bad & set(d.get("env", {})):
        print(f"offending settings: {p}", file=sys.stderr)
        sys.exit(1)
PY
fi
if [ "$DRY_RUN" -eq 0 ] && [ "$PICK_ONLY" -eq 0 ]; then
  command -v gh >/dev/null || die "'gh' not found; install the GitHub CLI"
  gh auth status >/dev/null 2>&1 || die "gh is not logged in; run 'gh auth login'"
fi
GH_FLAG=()
if ! command -v gh >/dev/null || ! gh auth status >/dev/null 2>&1; then
  [ "$DRY_RUN" -eq 1 ] || [ "$PICK_ONLY" -eq 1 ] || die "gh unavailable"
  log "gh unavailable: open-PR check skipped (remote branches are still checked)"
  GH_FLAG=(--no-gh)
fi

mkdir -p "$CACHE/worktrees" "$CACHE/logs" "$CACHE/target"
exec 9>"$CACHE/lock"
flock -n 9 || die "another game-support run is in progress ($CACHE/lock)"

# ---- pick ----------------------------------------------------------------
log "fetching origin"
git -C "$REPO" fetch --quiet --prune origin
git -C "$REPO" rev-parse --verify --quiet "origin/$BASE^{commit}" >/dev/null \
  || die "origin/$BASE does not exist"

WT="$CACHE/worktrees/$STAMP"
git -C "$REPO" worktree add --quiet --detach "$WT" "origin/$BASE"
KEEP_WT=1
cleanup() {
  if [ "${KEEP_WT:-1}" -eq 1 ] && [ -d "$WT" ]; then
    log "worktree kept for inspection: $WT"
    log "remove it with: git -C '$REPO' worktree remove --force '$WT'"
  fi
}
trap cleanup EXIT
remove_wt() { KEEP_WT=0; git -C "$REPO" worktree remove --force "$WT"; }

pick_json="$(cd "$WT" && python3 -I "$PICKER" --root "$WT" --game "$GAME" "${GH_FLAG[@]}")" \
  || { remove_wt; die "picker failed"; }
read_field() { python3 -I -c 'import json,sys; v=json.loads(sys.argv[1]).get(sys.argv[2]); print(v or "")' "$pick_json" "$1"; }
DOMAIN="$(read_field domain)"
NAME="$(read_field name)"
DEFINITION="$(read_field definition)"
BRANCH="$(read_field branch)"

if [ -z "$DOMAIN" ]; then
  log "no pending game; nothing to do"
  remove_wt
  exit 0
fi
log "picked $DOMAIN ($NAME) -> $BRANCH, base origin/$BASE"
if [ "$PICK_ONLY" -eq 1 ]; then
  echo "$pick_json"
  remove_wt
  exit 0
fi

git -C "$WT" switch --quiet -c "$BRANCH"
NOTE="docs/game-support/$DOMAIN.md"
LOG="$CACHE/logs/$STAMP-$DOMAIN.jsonl"
export CARGO_TARGET_DIR="$CACHE/target"

# ---- research and configure (claude, headless) ----------------------------
ALLOWED_TOOLS="Read,Write,Edit,Glob,Grep,WebSearch,WebFetch,Bash(cargo fmt:*),Bash(cargo clippy:*),Bash(cargo test:*),Bash(git diff:*),Bash(git status:*),Bash(ls:*),Bash(python3 $CHECKER:*)"
PROMPT="$(cat "$PROMPT_FILE")

---
Inputs for this run:
DOMAIN=$DOMAIN
NAME=$NAME
DEFINITION=$DEFINITION
BRANCH=$BRANCH
CHECKER=$CHECKER"

log "running claude (log: $LOG, limit $CLAUDE_TIMEOUT)"
set +e
(cd "$WT" && timeout --foreground "$CLAUDE_TIMEOUT" claude -p "$PROMPT" \
  --permission-mode dontAsk \
  --permission-prompts none \
  --tools "Read,Write,Edit,Glob,Grep,WebSearch,WebFetch,Bash" \
  --allowedTools "$ALLOWED_TOOLS" \
  --setting-sources project,local \
  --strict-mcp-config \
  --no-session-persistence \
  --output-format stream-json --verbose \
  </dev/null >"$LOG" 2>&1)
rc=$?
set -e
[ "$rc" -eq 0 ] || die "claude exited with $rc (see $LOG)"

# Belt and braces: the session must report no API key source.
python3 -I - "$LOG" <<'PY' || die "claude did not run on the subscription login (see $LOG)"
import json, sys
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("type") == "system" and m.get("subtype") == "init":
        src = m.get("apiKeySource", "none")
        print(f"apiKeySource={src}", file=sys.stderr)
        sys.exit(0 if src == "none" else 1)
sys.exit(0)
PY

# Warn (do not fail) when the agent never ran cargo test; the checks below
# still decide the outcome.
python3 -I - "$LOG" <<'PY' || log "warning: claude never ran 'cargo test' (see $LOG); relying on the runner's own checks"
import json, re, sys
# A Bash call counts only when one of its command segments starts with
# `cargo test` (not text inside a heredoc) and it was not permission-denied.
SEG = re.compile(r"(?:^|&&|\|\||;|\|)\s*cargo\s+test\b", re.M)
pending = set()
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    try:
        m = json.loads(line)
    except ValueError:
        continue
    msg = m.get("message") if isinstance(m, dict) else None
    content = msg.get("content") if isinstance(msg, dict) else None
    if not isinstance(content, list):
        continue
    for c in content:
        if not isinstance(c, dict):
            continue
        if c.get("type") == "tool_use" and c.get("name") == "Bash":
            cmd = (c.get("input") or {}).get("command", "")
            if isinstance(cmd, str) and "<<" not in cmd and SEG.search(cmd):
                pending.add(c.get("id"))
        elif c.get("type") == "tool_result" and c.get("tool_use_id") in pending:
            if "has been denied" not in str(c.get("content", "")):
                sys.exit(0)
sys.exit(1)
PY

# ---- verify ----------------------------------------------------------------
log "verifying the change"
cd "$WT"
mapfile -t changed < <(git status --porcelain=v1 -uall | cut -c4-)
[ "${#changed[@]}" -gt 0 ] || die "claude made no changes"
allowed=("$DEFINITION" "crates/vessel-core/src/games/definitions.rs" "$NOTE")
bad=0
for f in "${changed[@]}"; do
  ok=0
  for a in "${allowed[@]}"; do [ "$f" = "$a" ] && ok=1; done
  [ "$ok" -eq 1 ] || { echo "error: $f is not in the allowed file set" >&2; bad=1; }
done
[ "$bad" -eq 0 ] || die "changes outside the allowed files"

check_out="$(python3 -I "$CHECKER" --definition "$DEFINITION" --note "$NOTE")" \
  || die "check_game_support_note.py failed"
echo "$check_out" >&2
OUTCOME="$(sed -n 's/^outcome: \([a-z]*\) = .*/\1/p' <<<"$check_out")"
GAPS="$(sed -n '/^gaps:$/,$p' <<<"$check_out" | tail -n +2)"
if [ "$OUTCOME" = full ]; then
  LABEL="full support"; SUPPORT=tested
else
  LABEL="partial (Rust changes needed)"; SUPPORT=experimental
fi

cargo fmt --all -- --check
cargo clippy -p vessel-core --all-targets --locked -- -D warnings
cargo test -p vessel-core --locked

if [ "$DRY_RUN" -eq 1 ]; then
  log "dry run passed: $DOMAIN -> $LABEL; nothing committed or pushed"
  git status --short >&2
  exit 0
fi

# ---- commit, push, PR ------------------------------------------------------
BODY="$CACHE/logs/$STAMP-$DOMAIN-pr.md"
{
  echo "**Outcome:** $LABEL (support = \"$SUPPORT\")"
  echo
  echo "**Gaps (Rust changes needed):**"
  echo
  echo "$GAPS"
  echo
  if [ "$OUTCOME" = full ]; then
    echo "_Promotion to tested here is based on research, not hardware testing._"
  else
    echo "_Left experimental until the Rust changes above land._"
  fi
  echo
  echo "---"
  echo
  cat "$NOTE"
} >"$BODY"

git add -- "${changed[@]}"
git commit --quiet -m "games: $DOMAIN: $LABEL" \
  -m "Researched and generated by scripts/run-game-support.sh; see $NOTE."
git push --quiet -u origin "$BRANCH"
gh pr create --base "$BASE" --head "$BRANCH" \
  --title "games: $NAME ($DOMAIN): $LABEL" --body-file "$BODY"

cd "$REPO"
remove_wt
log "done: $DOMAIN -> $LABEL (log: $LOG)"
