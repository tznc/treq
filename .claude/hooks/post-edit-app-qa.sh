#!/usr/bin/env bash
# PostToolUse hook (Write|Edit|MultiEdit): after any edit to UI-affecting source,
# nudge Claude to run the /app-qa skill (gpui-playwright screenshot QA and
# gpui-testing-library screen tests) before reporting the change as done.
set -euo pipefail

input="$(cat)"
file_path="$(printf '%s' "$input" | jq -r '.tool_response.filePath // .tool_input.file_path // empty')"

if [ -z "$file_path" ]; then
	exit 0
fi

case "$file_path" in
*src-tauri/src/gpui_shell/* | *src-tauri/src/core/*)
	;;
*)
	exit 0
	;;
esac

marker="${TMPDIR:-/tmp}/treq-app-qa-reminder-$(printf '%s' "$input" | jq -r '.session_id // "none"')"
# Remind once per session: repeating the same reminder after every edit
# spends context without adding information.
if [ -e "$marker" ]; then
	exit 0
fi
touch "$marker"

rel_path="$file_path"
if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then
	rel_path="${file_path#"$CLAUDE_PROJECT_DIR"/}"
fi

context="UI-affecting file changed: ${rel_path}. Before telling the user this change is done, use the /app-qa skill: cover the behavior with a gpui-testing-library screen test, drive it in the real app with gpui-playwright (e2e/desktop/qa), and visually confirm the before/after screenshots."

jq -n --arg ctx "$context" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
