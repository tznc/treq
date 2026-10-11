#!/usr/bin/env bash
set -euo pipefail

# Docker creates new named volumes as root; let the development user build.
sudo chown "$(id -u):$(id -g)" node_modules target
npm ci

# Match the jj-lib version used by the backend. Tests also exercise the CLI.
if ! command -v jj >/dev/null || [[ "$(jj --version)" != "jj 0.40.0" ]]; then
  cargo install jj-cli --version 0.40.0 --locked
fi
