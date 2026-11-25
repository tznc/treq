// Which treq CLI release a machine's bootstrap installs. The newest
// published release (not a prerelease) in treq.dev/releases.json that has
// both Linux CLI archives wins, so machines keep working while the manifest's
// pinned version is still unreleased; an older release is used when a newer
// one is incomplete. Digests pinned by hand in a manifest beat the digests
// releases.json reports.
// releases.json is GitHub's release list, rebuilt by the website deploy
// (web/plugins/releasesJsonPlugin.js), so machines never call the GitHub API.

import { allManifests, type BootManifest, type TreqSha256 } from "./boot-manifest.ts";
import { ProviderError } from "./provider.ts";
import { errorMessage } from "../error-message.ts";

export interface ResolvedTreqRelease {
  version: string;
  sha256: TreqSha256;
  /** "manifest": reviewed pins; "github": the GitHub digests releases.json reports. */
  source: "manifest" | "github";
}

export interface TreqReleaseResolverOptions {
  fetch?: typeof fetch;
  now?: () => number;
  /** Manifests whose real digests are reviewed pins; the last is the current one. */
  manifests?: BootManifest[];
  /** TREQ_RELEASES_URL overrides where releases.json is read from (tests, staging). */
  env?: (key: string) => string | undefined;
}

const RELEASES_URL = "https://treq.dev/releases.json";
const CACHE_MS = 10 * 60_000;
const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;
const SHA256 = /^[0-9a-f]{64}$/;
const ARCHES = ["x86_64", "aarch64"] as const;

/** A release in treq.dev/releases.json, schema 1, before validation. */
interface FeedRelease {
  version?: unknown;
  tag?: unknown;
  prerelease?: unknown;
  assets?: Record<string, { sha256?: unknown } | null | undefined> | null;
}

const isPinned = (d: TreqSha256) => SHA256.test(d.x86_64) && SHA256.test(d.aarch64);

function reviewedPins(manifests: BootManifest[], version: string): TreqSha256 | null {
  return manifests.find((m) => m.treq_version === version && isPinned(m.treq_sha256))?.treq_sha256 ?? null;
}

/** Both Linux CLI digests, or null when either archive or its digest is missing or malformed. */
function feedDigests(release: FeedRelease): TreqSha256 | null {
  const digests: Partial<TreqSha256> = {};
  for (const arch of ARCHES) {
    const hex = release.assets?.[`linux-${arch}`]?.sha256;
    if (typeof hex !== "string" || !SHA256.test(hex)) return null;
    digests[arch] = hex;
  }
  return digests as TreqSha256;
}

/** Published releases with a plain x.y.z version and matching vX.Y.Z tag, newest first. */
function publishedNewestFirst(releases: unknown[]): { version: string; release: FeedRelease }[] {
  return releases
    .flatMap((entry) => {
      if (typeof entry !== "object" || entry === null) return [];
      const release = entry as FeedRelease;
      if (release.prerelease !== false || typeof release.version !== "string") return [];
      const m = SEMVER.exec(release.version);
      return m && release.tag === `v${release.version}` ? [{ release, parts: [Number(m[1]), Number(m[2]), Number(m[3])] as const }] : [];
    })
    .sort((a, b) => b.parts[0] - a.parts[0] || b.parts[1] - a.parts[1] || b.parts[2] - a.parts[2])
    .map(({ release, parts }) => ({ release, version: parts.join(".") }));
}

async function fromFeed(fetchFn: typeof fetch, url: string, manifests: BootManifest[]): Promise<ResolvedTreqRelease> {
  const response = await fetchFn(url, {
    headers: { accept: "application/json", "user-agent": "treq-control-plane" },
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error(`${url} answered ${response.status}`);
  }
  const doc = await response.json() as { schema?: unknown; releases?: unknown } | null;
  if (doc?.schema !== 1 || !Array.isArray(doc.releases)) {
    throw new Error(`${url} is not a schema 1 release list`);
  }
  for (const { version, release } of publishedNewestFirst(doc.releases)) {
    const pinned = reviewedPins(manifests, version);
    if (pinned) return { version, sha256: pinned, source: "manifest" };
    const digests = feedDigests(release);
    if (digests) return { version, sha256: digests, source: "github" };
  }
  throw new Error("no published release has both Linux treq-cli archives");
}

/** A resolver with its own in-process cache of the last releases.json answer. */
export function createTreqReleaseResolver(opts: TreqReleaseResolverOptions = {}): () => Promise<ResolvedTreqRelease> {
  const fetchFn = opts.fetch ?? fetch;
  const now = opts.now ?? Date.now;
  const manifests = opts.manifests ?? allManifests();
  const env = opts.env ?? ((key: string) => Deno.env.get(key));
  let cached: { at: number; value: ResolvedTreqRelease } | null = null;
  return async () => {
    if (cached && now() - cached.at < CACHE_MS) return cached.value;
    try {
      const value = await fromFeed(fetchFn, env("TREQ_RELEASES_URL") || RELEASES_URL, manifests);
      cached = { at: now(), value };
      return value;
    } catch (err) {
      // Without releases.json, only a reviewed pin for the current manifest is safe.
      const current = manifests[manifests.length - 1];
      if (current && isPinned(current.treq_sha256)) {
        return { version: current.treq_version, sha256: current.treq_sha256, source: "manifest" };
      }
      throw new ProviderError(
        "unavailable",
        `cannot resolve a treq CLI release to bootstrap: ${errorMessage(err)}; ` +
          `manifest treq ${current?.treq_version} has no pinned digests`,
      );
    }
  };
}

export const resolveTreqRelease = createTreqReleaseResolver();

/** The current manifest's own treq pins, unchecked: for the stub provider, which runs no script. */
export function manifestTreqRelease(): Promise<ResolvedTreqRelease> {
  const m = allManifests().at(-1);
  if (!m) throw new Error("no boot manifests");
  return Promise.resolve({ version: m.treq_version, sha256: m.treq_sha256, source: "manifest" });
}
