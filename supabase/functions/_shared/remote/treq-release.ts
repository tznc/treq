// Which treq CLI release a machine's bootstrap installs. The newest
// published GitHub release (not a draft or prerelease) that has both Linux
// CLI archives wins, so machines keep working while the manifest's pinned
// version is still unreleased; an older release is used when a newer one is
// incomplete. Digests reviewed into a manifest (scripts/bump-boot-manifest.ts)
// beat the digests GitHub reports for the same version.

import { allManifests, type BootManifest, type TreqSha256 } from "./boot-manifest.ts";
import { ProviderError } from "./provider.ts";

export interface ResolvedTreqRelease {
  version: string;
  sha256: TreqSha256;
  /** "manifest": reviewed pins; "github": the digests the release API reports. */
  source: "manifest" | "github";
}

export interface TreqReleaseResolverOptions {
  fetch?: typeof fetch;
  now?: () => number;
  /** Manifests whose real digests are reviewed pins; the last is the current one. */
  manifests?: BootManifest[];
}

const RELEASES_URL = "https://api.github.com/repos/tznc/treq/releases?per_page=50";
const CACHE_MS = 10 * 60_000;
const SEMVER = /^v(\d+)\.(\d+)\.(\d+)$/;
const SHA256 = /^[0-9a-f]{64}$/;
const ARCHES = ["x86_64", "aarch64"] as const;

interface GithubRelease {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  assets: { name: string; digest?: string | null }[];
}

const isPinned = (d: TreqSha256) => SHA256.test(d.x86_64) && SHA256.test(d.aarch64);

function reviewedPins(manifests: BootManifest[], version: string): TreqSha256 | null {
  return manifests.find((m) => m.treq_version === version && isPinned(m.treq_sha256))?.treq_sha256 ?? null;
}

/** Both Linux CLI digests, or null when either archive or its digest is missing. */
function apiDigests(release: GithubRelease, version: string): TreqSha256 | null {
  const digests: Partial<TreqSha256> = {};
  for (const arch of ARCHES) {
    const asset = (release.assets ?? []).find((a) => a.name === `treq-cli-${version}-linux-${arch}.tar.gz`);
    const hex = /^sha256:([0-9a-f]{64})$/.exec(asset?.digest ?? "")?.[1];
    if (!hex) return null;
    digests[arch] = hex;
  }
  return digests as TreqSha256;
}

/** Published releases with a plain x.y.z tag, newest first. */
function publishedNewestFirst(releases: GithubRelease[]): { version: string; release: GithubRelease }[] {
  return releases
    .flatMap((release) => {
      const m = release.draft || release.prerelease ? null : SEMVER.exec(release.tag_name);
      return m ? [{ release, parts: m.slice(1).map(Number) }] : [];
    })
    .sort((a, b) => b.parts[0] - a.parts[0] || b.parts[1] - a.parts[1] || b.parts[2] - a.parts[2])
    .map(({ release, parts }) => ({ release, version: parts.join(".") }));
}

async function fromGithub(fetchFn: typeof fetch, manifests: BootManifest[]): Promise<ResolvedTreqRelease> {
  const response = await fetchFn(RELEASES_URL, {
    headers: { accept: "application/vnd.github+json", "user-agent": "treq-control-plane" },
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error(`GitHub releases answered ${response.status}`);
  }
  for (const { version, release } of publishedNewestFirst(await response.json() as GithubRelease[])) {
    const pinned = reviewedPins(manifests, version);
    if (pinned) return { version, sha256: pinned, source: "manifest" };
    const digests = apiDigests(release, version);
    if (digests) return { version, sha256: digests, source: "github" };
  }
  throw new Error("no published release has both Linux treq-cli archives");
}

/** A resolver with its own in-process cache of the last GitHub answer. */
export function createTreqReleaseResolver(opts: TreqReleaseResolverOptions = {}): () => Promise<ResolvedTreqRelease> {
  const fetchFn = opts.fetch ?? fetch;
  const now = opts.now ?? Date.now;
  const manifests = opts.manifests ?? allManifests();
  let cached: { at: number; value: ResolvedTreqRelease } | null = null;
  return async () => {
    if (cached && now() - cached.at < CACHE_MS) return cached.value;
    try {
      const value = await fromGithub(fetchFn, manifests);
      cached = { at: now(), value };
      return value;
    } catch (err) {
      // Without GitHub, only a reviewed pin for the current manifest is safe.
      const current = manifests[manifests.length - 1];
      if (current && isPinned(current.treq_sha256)) {
        return { version: current.treq_version, sha256: current.treq_sha256, source: "manifest" };
      }
      throw new ProviderError(
        "unavailable",
        `cannot resolve a treq CLI release to bootstrap: ${(err as Error).message}; ` +
          `manifest treq ${current?.treq_version} has no pinned digests`,
      );
    }
  };
}

export const resolveTreqRelease = createTreqReleaseResolver();

/** The current manifest's own treq pins, unchecked: for the stub provider, which runs no script. */
export function manifestTreqRelease(): Promise<ResolvedTreqRelease> {
  const m = allManifests()[allManifests().length - 1];
  return Promise.resolve({ version: m.treq_version, sha256: m.treq_sha256, source: "manifest" });
}
