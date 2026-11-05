#!/usr/bin/env bash
# Checks `treq agent msg` end to end with the coding agents installed here.
#
# It starts worker agents and one orchestrator in fresh workspaces of a
# repository open in the Treq app, has the agents message each other with
# the `treq agent msg` CLI, then reads Treq's system log (and, when sqlite3
# is installed, the app db) to confirm every message was queued, delivered,
# read and acked or replied to.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/agent-msg-verify.sh [options]

Run it from inside a repository that is open in the Treq app.

Options:
  --repo PATH        Repository to use (default: the one containing the current directory)
  --harness NAME     Only this agent: claude, codex, cursor or copilot. Repeatable.
                     Default: every one installed. With only one, two of it run.
  --timeout SECONDS  How long to wait for the agents (default: 600)
  --log-dir DIR      Treq's log directory (default: the app's log directory)
  -h, --help         Show this help

The agents ask before running shell commands unless you allowed them: approve
each `treq agent msg` command when asked. A message waits at most a minute, so
approve promptly. Nothing in the repository is changed; the workspaces the
script creates are listed at the end for you to delete.
EOF
}

repo=""
harnesses=()
timeout=600
log_dir=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) repo="$2"; shift 2 ;;
    --harness) harnesses+=("$2"); shift 2 ;;
    --timeout) timeout="$2"; shift 2 ;;
    --log-dir) log_dir="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

fail() { echo "error: $*" >&2; exit 1; }

for tool in treq jq; do
  command -v "$tool" >/dev/null || fail "$tool is not on PATH"
done

if [[ -z "$repo" ]]; then
  repo="$(git rev-parse --show-toplevel 2>/dev/null || jj root 2>/dev/null || true)"
fi
[[ -n "$repo" && -d "$repo" ]] || fail "not in a repository; pass --repo"
cd "$repo"

if [[ -z "$log_dir" ]]; then
  case "$(uname -s)" in
    Darwin) log_dir="$HOME/Library/Logs/com.treq" ;;
    *) log_dir="${XDG_DATA_HOME:-$HOME/.local/share}/com.treq/logs" ;;
  esac
fi
[[ -d "$log_dir" ]] || fail "no Treq log directory at $log_dir; pass --log-dir"

# The harnesses treq launches, and the binary each one runs.
binary_of() {
  case "$1" in
    claude) echo claude ;;
    codex) echo codex ;;
    cursor) echo cursor-agent ;;
    copilot) echo copilot ;;
    *) return 1 ;;
  esac
}

if [[ ${#harnesses[@]} -eq 0 ]]; then
  for h in claude codex cursor copilot; do
    command -v "$(binary_of "$h")" >/dev/null && harnesses+=("$h")
  done
fi
[[ ${#harnesses[@]} -gt 0 ]] || fail "none of claude, codex, cursor-agent or copilot is installed"
for h in "${harnesses[@]}"; do
  binary_of "$h" >/dev/null || fail "unknown harness '$h' (use claude, codex, cursor or copilot)"
  command -v "$(binary_of "$h")" >/dev/null || fail "$h is not installed ($(binary_of "$h") is not on PATH)"
done
# Messaging needs two agents: with one harness, two of it run.
[[ ${#harnesses[@]} -eq 1 ]] && harnesses+=("${harnesses[0]}")

treq agent msg --list >/dev/null || fail "the Treq app is not running with $repo open"

run_id="msgv$(date +%s)"
echo "treq agent msg check $run_id in $repo"
echo "agents: ${harnesses[*]}"
echo

# Only log lines written from now on count.
log_offsets="$(mktemp)"
trap 'rm -f "$log_offsets"' EXIT
for file in "$log_dir"/treq.*; do
  [[ -f "$file" ]] && echo "$file $(wc -c <"$file")" >>"$log_offsets"
done
new_log_lines() {
  local file start
  for file in "$log_dir"/treq.*; do
    [[ -f "$file" ]] || continue
    start="$(awk -v f="$file" '$1 == f { print $2 }' "$log_offsets")"
    tail -c +"$(( ${start:-0} + 1 ))" "$file"
  done | grep -a "event=" || true
}

workspaces=()
start_agent() {
  local harness="$1" workspace="$2" prompt="$3"
  treq add "$workspace" -d "treq agent msg check" >/dev/null
  treq agent start "$workspace" "$prompt" --agent "$harness" -m edit >/dev/null
  workspaces+=("$workspace")
}

# The handle of the agent running in `workspace`, once it is up.
handle_in() {
  local workspace="$1" deadline=$(( SECONDS + 120 )) handle=""
  while [[ -z "$handle" && $SECONDS -lt $deadline ]]; do
    handle="$(treq agent msg --list --json 2>/dev/null \
      | jq -r --arg ws "$workspace" '.agents[] | select(.workspace == $ws) | .handle' | head -n1)"
    [[ -n "$handle" ]] || sleep 2
  done
  [[ -n "$handle" ]] || fail "no agent came up in $workspace"
  echo "$handle"
}

worker_prompt='This is a check of treq agent messaging. Change no files. Using the shell, loop:
1. Run: treq agent msg --inbox --wait --json
2. For each message in the output: if its "kind" is "ask", run: treq agent msg --reply <id> "pong"; otherwise run: treq agent msg --ack <id>.
3. After you handle a message whose text starts with "broadcast", run: treq agent msg --status done --note verified
Stop once you have handled two messages.'

# Every harness but the last is a worker; the last orchestrates.
orchestrator_harness="${harnesses[${#harnesses[@]}-1]}"
workers=()
for i in "${!harnesses[@]}"; do
  [[ $i -eq $(( ${#harnesses[@]} - 1 )) ]] && break
  workspace="$run_id-$i-${harnesses[$i]}"
  start_agent "${harnesses[$i]}" "$workspace" "$worker_prompt"
  handle="$(handle_in "$workspace")"
  workers+=("$handle")
  echo "worker  $handle (${harnesses[$i]})"
done
worker_list="${workers[*]}"
orchestrator_prompt="This is a check of treq agent messaging. Change no files. Using the shell, run these in order and show each output:
1. treq agent msg --list
2. For each of these agents: ${worker_list}. Run: treq agent msg <agent> \"ping\" --ask --json
3. treq agent msg --all \"broadcast $run_id\"
4. Run: treq agent msg --inbox --wait --types status --json. Repeat (at most three times) until you have a status message from each of: ${worker_list}.
5. For each status message, run: treq agent msg --ack <id>"
orchestrator_ws="$run_id-o-$orchestrator_harness"
start_agent "$orchestrator_harness" "$orchestrator_ws" "$orchestrator_prompt"
orchestrator="$(handle_in "$orchestrator_ws")"
echo "orchestrator $orchestrator ($orchestrator_harness)"
echo
echo "Approve each treq agent msg command the agents ask to run."
echo "Waiting up to ${timeout}s for the agents…"

# One row per (message, target): `id kind from to`, from the queued lines.
sent_rows() {
  new_log_lines | sed -n 's/.*event=queued id=\([^ "]*\) kind=\([^ "]*\) from=\([^ "]*\) to=\([^ "]*\).*/\1 \2 \3 \4/p' | sort -u
}
state_of() { # id target -> the furthest state logged for it
  local id="$1" to="$2" lines
  lines="$(new_log_lines | grep -a "id=$id " | grep -aE "to=$to( |\"|$)" || true)"
  for state in replied acked timed_out read delivered queued; do
    if grep -aq "event=$state " <<<"$lines"; then echo "$state"; return; fi
  done
  echo "-"
}
mode_of() {
  new_log_lines | grep -a "event=delivered id=$1 " | grep -aE "to=$2( |\"|$)" | sed -n 's/.* mode=\([^ "]*\).*/\1/p' | head -n1
}

# Expected traffic: an ask to each worker and its reply, the broadcast to
# each worker, and a status from each worker.
expected=$(( ${#workers[@]} * 4 ))
deadline=$(( SECONDS + timeout ))
while [[ $SECONDS -lt $deadline ]]; do
  done_count=0
  while read -r id _ _ to; do
    [[ -n "$id" ]] || continue
    case "$(state_of "$id" "$to")" in acked|replied|timed_out) done_count=$(( done_count + 1 )) ;; esac
  done < <(sent_rows)
  [[ $done_count -ge $expected ]] && break
  sleep 5
done

echo
printf '%-12s %-8s %-14s %-14s %-10s %-10s %s\n' KIND ID FROM TO DELIVERED FINAL CHECK
failures=0
checked=0
check_row() { # kind from to: one expected message
  local want_kind="$1" want_from="$2" want_to="$3" row id state mode result
  row="$(sent_rows | awk -v k="$want_kind" -v f="$want_from" -v t="$want_to" '$2 == k && $3 == f && $4 == t { print; exit }')"
  checked=$(( checked + 1 ))
  if [[ -z "$row" ]]; then
    printf '%-12s %-8s %-14s %-14s %-10s %-10s %s\n' "$want_kind" "-" "$want_from" "$want_to" "-" "-" "FAIL (never sent)"
    failures=$(( failures + 1 ))
    return
  fi
  id="$(awk '{ print $1 }' <<<"$row")"
  state="$(state_of "$id" "$want_to")"
  mode="$(mode_of "$id" "$want_to")"
  case "$want_kind:$state" in
    ask:replied|message:acked|status:acked|reply:acked) result="ok" ;;
    *) result="FAIL"; failures=$(( failures + 1 )) ;;
  esac
  printf '%-12s %-8s %-14s %-14s %-10s %-10s %s\n' "$want_kind" "${id:0:8}" "$want_from" "$want_to" "${mode:--}" "$state" "$result"
}
for worker in "${workers[@]}"; do
  check_row ask "$orchestrator" "$worker"
  check_row reply "$worker" "$orchestrator"
  check_row message "$orchestrator" "$worker"
  check_row status "$worker" "$orchestrator"
done

# The app db must agree with the log.
db="$repo/.treq/local.db"
if command -v sqlite3 >/dev/null && [[ -f "$db" ]]; then
  ids="$(sent_rows | awk '{ printf "%s'\''%s'\''", (NR > 1 ? "," : ""), $1 }')"
  if [[ -n "$ids" ]]; then
    echo
    echo "app db ($db):"
    sqlite3 -column -header "$db" \
      "SELECT id, target, kind, state, mode FROM agent_messages WHERE id IN ($ids) ORDER BY sent_at, target;"
  fi
fi

echo
echo "workspaces created: ${workspaces[*]}"
echo "To check the typing guard by hand: type (without Enter) into a worker's prompt,"
echo "then have the orchestrator message it. The terminal shows a banner and treq types nothing."
echo
if [[ $failures -gt 0 ]]; then
  echo "$checked checks, $failures failed"
  exit 1
fi
echo "$checked checks passed"
