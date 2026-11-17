#!/usr/bin/env bash
# PostToolUse hook (Write|Edit|MultiEdit): after any edit to the Supabase
# service contract or the app code that calls it, nudge Claude to run the
# /service-qa skill (local Supabase CLI harness) before reporting the change
# as done.
set -euo pipefail

input="$(cat)"
file_path="$(printf '%s' "$input" | jq -r '.tool_response.filePath // .tool_input.file_path // empty')"

if [ -z "$file_path" ]; then
	exit 0
fi

case "$file_path" in
*supabase/migrations/* | *supabase/functions/* | *supabase/config.toml | \
*src/lib/supabase.ts | *src/stores/authStore.ts | \
*web/src/lib/supabase.ts | *web/src/pages/sign-in.tsx | *web/src/pages/auth/* | \
*web/src/pages/integrations/github/* | *web/src/components/AuthProvider.tsx | \
*src/components/GitHubIntegrationSettings.tsx | *src/components/GitHubPanel.tsx)
	;;
*)
	exit 0
	;;
esac

marker="${TMPDIR:-/tmp}/treq-service-qa-reminder-$(printf '%s' "$input" | jq -r '.session_id // "none"')"
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

context="Supabase-backed file changed: ${rel_path}. Before telling the user this change is done, use the /service-qa skill to exercise the affected Auth/RPC/Edge contract against the local Supabase CLI stack (scripts/service-qa/, real HTTP — never mock src/lib/supabase), and confirm each recorded outcome in scripts/service-qa/.generated/."

jq -n --arg ctx "$context" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
