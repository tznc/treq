// Boot manifest registry, kept as a
// small static table (not fetched from the vendor) so the control plane can
// pin exactly which dependency versions a given `manifest_version` installs.

export interface BootManifestAgent {
  name: string;
  version: string;
}

/**
 * SHA-256 of the treq CLI release archive per arch, pinned here so the boot
 * script does not trust a checksum served by the same release. A value
 * starting with PLACEHOLDER makes bootstrap fail closed; ops replace it with
 * the real digest when the release ships.
 */
export interface TreqSha256 {
  x86_64: string;
  aarch64: string;
}

export interface BootManifest {
  manifest_version: number;
  treq_version: string;
  treq_sha256: TreqSha256;
  jj_version: string;
  jj_sha256: TreqSha256;
  git_version: string;
  agents: BootManifestAgent[];
}

export const CURRENT_MANIFEST_VERSION = 4;

// sha256 of jj-v0.40.0-<arch>-unknown-linux-musl.tar.gz.
const JJ_0_40_SHA256: TreqSha256 = {
  x86_64: "5c8979f46873e052f59bdd9535636dca6e6f9f70571b73f6d63c3b92acfaa037",
  aarch64: "b26f24ff7a34838fbafe8788e6a94a9cdcf51601ef8c9af8fab4fa22c06ddbee",
};

const MANIFESTS: Record<number, BootManifest> = {
  1: {
    manifest_version: 1,
    treq_version: "0.3.0",
    treq_sha256: { x86_64: "PLACEHOLDER-treq-0.3.0-x86_64", aarch64: "PLACEHOLDER-treq-0.3.0-aarch64" },
    jj_version: "0.24.0",
    jj_sha256: { x86_64: "PLACEHOLDER-jj-0.24.0-x86_64", aarch64: "PLACEHOLDER-jj-0.24.0-aarch64" },
    git_version: "2.45",
    agents: [
      { name: "claude", version: "1.0.0" },
      { name: "codex", version: "1.0.0" },
    ],
  },
  // jj matches the jj-lib treq links (src-tauri/Cargo.toml): jj 0.24 cannot
  // open repositories that treq has written.
  2: {
    manifest_version: 2,
    treq_version: "0.3.0",
    treq_sha256: { x86_64: "PLACEHOLDER-treq-0.3.0-x86_64", aarch64: "PLACEHOLDER-treq-0.3.0-aarch64" },
    jj_version: "0.40.0",
    jj_sha256: JJ_0_40_SHA256,
    git_version: "2.45",
    agents: [
      { name: "claude", version: "1.0.0" },
      { name: "codex", version: "1.0.0" },
    ],
  },
  // treq 0.4.0 is the first release with `treq gateway`, which the machine
  // runs as its SSH gateway (machine-setup.ts provisionGateway).
  3: {
    manifest_version: 3,
    treq_version: "0.4.0",
    // The release workflow pins these (scripts/bump-boot-manifest.ts).
    treq_sha256: { x86_64: "PLACEHOLDER-treq-0.4.0-x86_64", aarch64: "PLACEHOLDER-treq-0.4.0-aarch64" },
    jj_version: "0.40.0",
    jj_sha256: JJ_0_40_SHA256,
    git_version: "2.45",
    agents: [
      { name: "claude", version: "1.0.0" },
      { name: "codex", version: "1.0.0" },
    ],
  },
  // Same pins as 3. Scripts up to 3 only echoed the jj and git installs yet
  // recorded the version as done; 4 makes those machines run the real ones.
  4: {
    manifest_version: 4,
    treq_version: "0.4.0",
    // The release workflow pins these (scripts/bump-boot-manifest.ts).
    treq_sha256: { x86_64: "PLACEHOLDER-treq-0.4.0-x86_64", aarch64: "PLACEHOLDER-treq-0.4.0-aarch64" },
    jj_version: "0.40.0",
    jj_sha256: JJ_0_40_SHA256,
    git_version: "2.45",
    agents: [
      { name: "claude", version: "1.0.0" },
      { name: "codex", version: "1.0.0" },
    ],
  },
};

export function manifestForVersion(version: number): BootManifest | null {
  return MANIFESTS[version] ?? null;
}

// The exec entrypoint installed as a Fly machine's `init.exec`, so a
// machine created through this Edge Function boots with an idempotent
// bootstrap script.
export function bootstrapCommand(manifestVersion: number): string[] {
  return ["/bin/sh", "-c", bootstrapScript(manifestVersion)];
}

export function bootstrapScript(requestedVersion: number): string {
  const requested = manifestForVersion(requestedVersion);
  // An unknown version falls back to the current manifest's contents *and*
  // version number, so the rendered script stays internally consistent.
  const manifestVersion = requested ? requestedVersion : CURRENT_MANIFEST_VERSION;
  const manifest = requested ?? MANIFESTS[CURRENT_MANIFEST_VERSION];
  const agentLines = manifest.agents
    .map((agent) => `install_agent "${agent.name}" "${agent.version}"\n`)
    .join("");

  return `#!/bin/sh
# Treq boot manifest bootstrap, generation-tied version ${manifestVersion}.
# Idempotent: safe to re-run on wake or reprovision without duplicating work.
set -eu

TREQ_MANIFEST_VERSION="${manifestVersion}"
TREQ_VERSION="${manifest.treq_version}"
TREQ_SHA256_X86_64="${manifest.treq_sha256.x86_64}"
TREQ_SHA256_AARCH64="${manifest.treq_sha256.aarch64}"
JJ_VERSION="${manifest.jj_version}"
JJ_SHA256_X86_64="${manifest.jj_sha256.x86_64}"
JJ_SHA256_AARCH64="${manifest.jj_sha256.aarch64}"
GIT_VERSION="${manifest.git_version}"
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

install_treq() {
  # The gateway (\`treq gateway\`) and the remote PTY supervisor run this binary.
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
  # \`jj --version\` prints \`jj <version>-<commit>\`.
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

install_treq
install_jj
install_git
${agentLines}
# Reached only when every real install above succeeded (set -e).
echo "$TREQ_MANIFEST_VERSION" | $SUDO tee "$STATE_FILE" >/dev/null
echo "treq bootstrap: manifest version $TREQ_MANIFEST_VERSION installed"
`;
}
