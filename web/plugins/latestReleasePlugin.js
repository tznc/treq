// @ts-check
const fs = require('fs');
const path = require('path');

const LATEST_RELEASE_API =
  'https://api.github.com/repos/tznc/treq/releases/latest';

// GitHub's /releases/latest skips drafts and prereleases, so this is the
// release the desktop updater can actually download.
async function fetchLatestRelease() {
  /** @type {Record<string, string>} */
  const headers = {Accept: 'application/vnd.github+json'};
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }
  const res = await fetch(LATEST_RELEASE_API, {
    headers,
    signal: AbortSignal.timeout(15_000),
  });
  if (!res.ok) throw new Error(`GitHub API returned ${res.status}`);
  const release = await res.json();
  const version = String(release.tag_name).replace(/^v/, '');
  // The updater only parses plain X.Y.Z.
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`unexpected release tag ${release.tag_name}`);
  }
  return {
    tag: release.tag_name,
    version,
    htmlUrl: release.html_url,
    assets: release.assets.map((/** @type {any} */ a) => ({
      name: a.name,
      url: a.browser_download_url,
    })),
  };
}

// Writes static/version (read by the macOS updater via treq.dev/version) and
// exposes the latest release to the client as global data.
/** @type {import('@docusaurus/types').PluginModule} */
function latestReleasePlugin(context) {
  return {
    name: 'latest-release-plugin',
    async loadContent() {
      const dest = path.join(context.siteDir, 'static', 'version');
      try {
        const release = await fetchLatestRelease();
        fs.writeFileSync(dest, `${release.version}\n`, 'utf8');
        return release;
      } catch (err) {
        // Fallback rule: a production deploy build (DOCUSAURUS_ENABLE_GTAG=true,
        // set only for the main-branch deploy) fails, because shipping a wrong
        // /version sends every macOS user a broken update prompt. Any other
        // build (local, PR preview, CI checks) carries on without /version:
        // the old file is deleted so a stale value can't ship, and download
        // links fall back to the release page.
        const reason = err instanceof Error ? err.message : String(err);
        if (process.env.DOCUSAURUS_ENABLE_GTAG === 'true') {
          throw new Error(`[latest-release] cannot fetch latest release: ${reason}`);
        }
        fs.rmSync(dest, {force: true});
        console.warn(
          `[latest-release] cannot fetch latest release (${reason}); building without /version.`,
        );
        return null;
      }
    },
    async contentLoaded({content, actions}) {
      actions.setGlobalData({release: content});
    },
  };
}

module.exports = latestReleasePlugin;
