// @ts-check
const fs = require('node:fs');
const path = require('node:path');

const RELEASES_API = 'https://api.github.com/repos/tznc/treq/releases';
const PER_PAGE = 100;
const ARCHES = ['x86_64', 'aarch64'];

/**
 * Every release GitHub lists, page by page. Drafts are only visible with a
 * token and are dropped later by toReleasesJson.
 * @param {typeof fetch} fetchFn
 * @returns {Promise<any[]>}
 */
async function fetchAllReleases(fetchFn = fetch) {
  /** @type {Record<string, string>} */
  const headers = {Accept: 'application/vnd.github+json'};
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }
  /** @type {any[]} */
  const all = [];
  // 50 pages is 5,000 releases: a guard against a looping API, not a limit.
  for (let page = 1; page <= 50; page++) {
    const res = await fetchFn(`${RELEASES_API}?per_page=${PER_PAGE}&page=${page}`, {
      headers,
      signal: AbortSignal.timeout(15_000),
    });
    if (!res.ok) throw new Error(`GitHub API returned ${res.status}`);
    const batch = await res.json();
    all.push(...batch);
    if (batch.length < PER_PAGE) return all;
  }
  throw new Error('too many release pages');
}

/**
 * GitHub's release list as treq.dev/releases.json, schema 1:
 *
 *   {schema: 1, generated_at, releases: [{version, tag, published_at,
 *     prerelease, assets: {'linux-x86_64': {url, sha256}, 'linux-aarch64': ...}}]}
 *
 * Every non-draft release, newest published first. `version` is the tag
 * without its leading "v" (the nightly's is "nightly"), so consumers check
 * its format and `prerelease` themselves. `assets` holds only the Linux CLI
 * tarballs the release has; `sha256` is the bare hex digest GitHub reports,
 * or null when GitHub has none.
 * @param {any[]} releases
 * @param {string} generatedAt
 */
function toReleasesJson(releases, generatedAt) {
  const out = releases
    .filter((r) => !r.draft)
    .map((r) => {
      const tag = String(r.tag_name);
      const version = tag.replace(/^v/, '');
      /** @type {Record<string, {url: string, sha256: string | null}>} */
      const assets = {};
      for (const arch of ARCHES) {
        const a = (r.assets ?? []).find(
          (/** @type {any} */ a) => a.name === `treq-cli-${version}-linux-${arch}.tar.gz`,
        );
        if (!a) continue;
        const hex = /^sha256:([0-9a-f]{64})$/.exec(a.digest ?? '')?.[1] ?? null;
        assets[`linux-${arch}`] = {url: a.browser_download_url, sha256: hex};
      }
      return {version, tag, published_at: r.published_at, prerelease: Boolean(r.prerelease), assets};
    })
    .sort((a, b) => String(b.published_at ?? '').localeCompare(String(a.published_at ?? '')));
  return {schema: 1, generated_at: generatedAt, releases: out};
}

// Writes static/releases.json (treq.dev/releases.json), the release history
// the cloud control plane reads instead of calling the GitHub API.
/**
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<void>}
 */
function releasesJsonPlugin(context) {
  return {
    name: 'releases-json-plugin',
    async loadContent() {
      const dest = path.join(context.siteDir, 'static', 'releases.json');
      try {
        const json = toReleasesJson(await fetchAllReleases(), new Date().toISOString());
        fs.writeFileSync(dest, `${JSON.stringify(json, null, 2)}\n`, 'utf8');
      } catch (err) {
        // Same rule as latestReleasePlugin: the production deploy build
        // (DOCUSAURUS_ENABLE_GTAG=true) fails rather than ship a stale or
        // missing release list; any other build deletes the old file and
        // carries on without it.
        if (process.env.DOCUSAURUS_ENABLE_GTAG === 'true') {
          throw new Error('[releases-json] cannot fetch releases', {cause: err});
        }
        fs.rmSync(dest, {force: true});
        console.warn(
          '[releases-json] cannot fetch releases; building without /releases.json:',
          err instanceof Error ? err.message : err,
        );
      }
    },
  };
}

module.exports = releasesJsonPlugin;
module.exports.toReleasesJson = toReleasesJson;
module.exports.fetchAllReleases = fetchAllReleases;
