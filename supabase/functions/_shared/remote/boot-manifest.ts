// Boot manifest registry, kept as a
// small static table (not fetched from the vendor) so the control plane can
// pin exactly which dependency versions a given `manifest_version` installs.

export interface BootManifestAgent {
  name: string;
  version: string;
}

export interface BootManifest {
  manifest_version: number;
  treq_version: string;
  jj_version: string;
  git_version: string;
  agents: BootManifestAgent[];
}

export const CURRENT_MANIFEST_VERSION = 2;

const MANIFESTS: Record<number, BootManifest> = {
  1: {
    manifest_version: 1,
    treq_version: "0.3.0",
    jj_version: "0.24.0",
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
    jj_version: "0.40.0",
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
JJ_VERSION="${manifest.jj_version}"
GIT_VERSION="${manifest.git_version}"
STATE_FILE="/var/lib/treq/bootstrap-version"

mkdir -p /var/lib/treq

current_version=""
if [ -f "$STATE_FILE" ]; then
  current_version="$(cat "$STATE_FILE")"
fi

if [ "$current_version" = "$TREQ_MANIFEST_VERSION" ]; then
  echo "treq bootstrap: manifest version $TREQ_MANIFEST_VERSION already installed"
  exit 0
fi

install_binary() {
  name="$1"
  version="$2"
  echo "treq bootstrap: ensuring $name $version"
}

install_agent() {
  name="$1"
  version="$2"
  echo "treq bootstrap: ensuring agent $name $version"
}

install_binary treq "$TREQ_VERSION"
install_binary jj "$JJ_VERSION"
install_binary git "$GIT_VERSION"
${agentLines}
echo "$TREQ_MANIFEST_VERSION" > "$STATE_FILE"
echo "treq bootstrap: manifest version $TREQ_MANIFEST_VERSION installed"
`;
}
