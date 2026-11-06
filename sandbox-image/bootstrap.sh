#!/bin/sh
# Treq boot manifest bootstrap, generation-tied version 5.
# Idempotent: safe to re-run on wake or reprovision without duplicating work.
set -eu

TREQ_MANIFEST_VERSION="5"
TREQ_VERSION="0.4.0"
TREQ_SHA256_X86_64="PLACEHOLDER-treq-0.4.0-x86_64"
TREQ_SHA256_AARCH64="PLACEHOLDER-treq-0.4.0-aarch64"
JJ_VERSION="0.40.0"
JJ_SHA256_X86_64="5c8979f46873e052f59bdd9535636dca6e6f9f70571b73f6d63c3b92acfaa037"
JJ_SHA256_AARCH64="b26f24ff7a34838fbafe8788e6a94a9cdcf51601ef8c9af8fab4fa22c06ddbee"
GIT_VERSION="2.45"
STATE_FILE="/var/lib/treq/bootstrap-version"

# Boat and Daytona exec as a non-root user.
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi

$SUDO mkdir -p /var/lib/treq

current_version=""
if [ -f "$STATE_FILE" ]; then
  current_version="$(cat "$STATE_FILE")"
fi

if [ "$current_version" = "$TREQ_MANIFEST_VERSION" ]; then
  echo "treq bootstrap: manifest version $TREQ_MANIFEST_VERSION already installed"
  exit 0
fi

# Agents are not installed by this script yet. The state file records only
# the treq, jj and git installs, so this step never marks anything done.
install_agent() {
  name="$1"
  version="$2"
  echo "treq bootstrap: agent $name $version is not installed by bootstrap"
}

machine_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo x86_64 ;;
    aarch64|arm64) echo aarch64 ;;
    *) echo "treq bootstrap: unsupported arch $(uname -m)" >&2; exit 1 ;;
  esac
}

install_base_packages() {
  # Only what is missing, so an image that already has them makes no apt call.
  missing=""
  for pkg in curl ca-certificates tar git openssh-server sudo; do
    dpkg -s "$pkg" >/dev/null 2>&1 || missing="$missing $pkg"
  done
  if [ -z "$missing" ]; then
    echo "treq bootstrap: base packages already installed"
    return 0
  fi
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get update -qq
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $missing
  echo "treq bootstrap: installed$missing"
}

install_treq() {
  # The gateway (`treq gateway`) and the remote PTY supervisor run this binary.
  if [ "$(/usr/local/bin/treq --version 2>/dev/null)" = "treq $TREQ_VERSION" ]; then
    echo "treq bootstrap: treq $TREQ_VERSION already installed"
    return 0
  fi
  case "$(uname -m)" in
    x86_64|amd64) ARCH=x86_64; expected="$TREQ_SHA256_X86_64" ;;
    aarch64|arm64) ARCH=aarch64; expected="$TREQ_SHA256_AARCH64" ;;
    *) echo "treq bootstrap: unsupported arch $(uname -m)" >&2; exit 1 ;;
  esac
  # The digest is pinned in the manifest, not read from the release.
  case "$expected" in
    PLACEHOLDER*) echo "treq bootstrap: no pinned sha256 for treq $TREQ_VERSION $ARCH" >&2; exit 1 ;;
  esac
  url="https://github.com/tznc/treq/releases/download/v$TREQ_VERSION/treq-cli-$TREQ_VERSION-linux-$ARCH.tar.gz"
  tmp="$(mktemp -d)"
  curl -fsSL --retry 3 -o "$tmp/treq.tar.gz" "$url"
  echo "$expected  $tmp/treq.tar.gz" | sha256sum -c -
  $SUDO tar -xzf "$tmp/treq.tar.gz" -C /usr/local/bin treq
  rm -rf "$tmp"
  echo "treq bootstrap: installed treq $TREQ_VERSION"
}

install_jj() {
  # `jj --version` prints `jj <version>-<commit>`.
  case "$(/usr/local/bin/jj --version 2>/dev/null)" in "jj $JJ_VERSION"|"jj $JJ_VERSION-"*)
    echo "treq bootstrap: jj $JJ_VERSION already installed"
    return 0 ;;
  esac
  ARCH="$(machine_arch)"
  if [ "$ARCH" = x86_64 ]; then expected="$JJ_SHA256_X86_64"; else expected="$JJ_SHA256_AARCH64"; fi
  case "$expected" in
    PLACEHOLDER*) echo "treq bootstrap: no pinned sha256 for jj $JJ_VERSION $ARCH" >&2; exit 1 ;;
  esac
  url="https://github.com/jj-vcs/jj/releases/download/v$JJ_VERSION/jj-v$JJ_VERSION-$ARCH-unknown-linux-musl.tar.gz"
  tmp="$(mktemp -d)"
  curl -fsSL --retry 3 -o "$tmp/jj.tar.gz" "$url"
  echo "$expected  $tmp/jj.tar.gz" | sha256sum -c -
  $SUDO tar -xzf "$tmp/jj.tar.gz" -C /usr/local/bin ./jj
  rm -rf "$tmp"
  echo "treq bootstrap: installed jj $JJ_VERSION"
}

install_git() {
  # The distro's git; the manifest's git_version ($GIT_VERSION) is a floor
  # the images already meet, not an exact pin.
  if command -v git >/dev/null 2>&1; then
    echo "treq bootstrap: $(git --version) already installed"
    return 0
  fi
  $SUDO apt-get update -qq
  $SUDO apt-get install -y git
  echo "treq bootstrap: installed $(git --version)"
}

install_base_packages
install_treq
install_jj
install_git
install_agent "claude" "1.0.0"
install_agent "codex" "1.0.0"

# Reached only when every real install above succeeded (set -e).
echo "$TREQ_MANIFEST_VERSION" | $SUDO tee "$STATE_FILE" >/dev/null
echo "treq bootstrap: manifest version $TREQ_MANIFEST_VERSION installed"
