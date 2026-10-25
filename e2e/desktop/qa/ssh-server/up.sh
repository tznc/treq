#!/usr/bin/env bash
# Starts the SSH host for 18-ssh-remote.spec.ts in Docker and prints the
# QA_SSH_* variables to export. Needs the headless CLI built first:
#   cargo build --manifest-path src-tauri/Cargo.toml --no-default-features \
#     --bin treq --target-dir target/cli
# It must run on the host's libc (or build the musl target instead).
# Env: QA_SSH_PORT (2222), QA_SSH_CLI (target/cli/debug/treq), JJ_VERSION.
# QA_SSH_CA: a CA bundle the host must trust, for a TLS-intercepting proxy
# in front of the spec's agent CLI installs.
# QA_SSH_PROXY: an HTTPS proxy on this machine's loopback the host must use
# (the container then shares this machine's network, sshd on QA_SSH_PORT).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
port="${QA_SSH_PORT:-2222}"
cli="${QA_SSH_CLI:-$root/target/cli/debug/treq}"
jj_version="${JJ_VERSION:-0.40.0}"
work="${TMPDIR:-/tmp}/treq-qa-ssh"
name=treq-qa-ssh

[ -x "$cli" ] || { echo "no treq CLI at $cli (see the header of $0)" >&2; exit 1; }
mkdir -p "$work"
[ -f "$work/id_ed25519" ] || ssh-keygen -q -t ed25519 -N "" -C treq-qa -f "$work/id_ed25519"
if [ ! -x "$work/jj" ]; then
  curl -fsSL "https://github.com/jj-vcs/jj/releases/download/v$jj_version/jj-v$jj_version-x86_64-unknown-linux-musl.tar.gz" \
    | tar xz -C "$work" ./jj
fi

docker build -q -t "$name" "$here" >/dev/null
docker rm -f "$name" >/dev/null 2>&1 || true
if [ -n "${QA_SSH_PROXY:-}" ]; then
  docker run -d --name "$name" --network host "$name" /usr/sbin/sshd -D -e -p "$port" >/dev/null
  docker exec "$name" sh -c "printf 'HTTPS_PROXY=%s\\nhttps_proxy=%s\\n' '$QA_SSH_PROXY' '$QA_SSH_PROXY' >> /etc/environment"
else
  docker run -d --name "$name" -p "127.0.0.1:$port:22" "$name" >/dev/null
fi
docker cp "$work/id_ed25519.pub" "$name:/home/dev/.ssh/authorized_keys"
docker cp "$cli" "$name:/usr/local/bin/treq"
docker cp "$work/jj" "$name:/usr/local/bin/jj"
docker exec "$name" sh -c 'chown dev:dev /home/dev/.ssh/authorized_keys && chmod 600 /home/dev/.ssh/authorized_keys'
if [ -n "${QA_SSH_CA:-}" ]; then
  docker cp "$QA_SSH_CA" "$name:/usr/local/share/ca-certificates/qa-proxy.crt"
  # sshd sessions read /etc/environment, not the container's env.
  docker exec "$name" sh -c 'update-ca-certificates >/dev/null 2>&1 && echo NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt >> /etc/environment'
fi

cat <<VARS
export QA_SSH_HOST=127.0.0.1 QA_SSH_PORT=$port QA_SSH_USER=dev
export QA_SSH_KEY=$work/id_ed25519 QA_SSH_REPO=/home/dev/qa-demo
VARS
