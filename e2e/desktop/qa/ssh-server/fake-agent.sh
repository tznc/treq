#!/usr/bin/env bash
# Stand-in for an agent CLI (claude, codex, cursor-agent, copilot), installed
# under each name: it says which agent it is and echoes every line it reads,
# so a spec can see the agent started in the workspace and got its input.
name="$(basename "$0")"
echo "$name agent ready in $PWD"
while IFS= read -r line; do echo "$name got: $line"; done
