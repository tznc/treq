// @ts-check
const {toReleasesJson, fetchAllReleases} = require('./releasesJsonPlugin');

const X86 = 'a'.repeat(64);
const ARM = 'b'.repeat(64);
const dl = (/** @type {string} */ tag, /** @type {string} */ name) =>
  `https://github.com/tznc/treq/releases/download/${tag}/${name}`;

/** A release as GitHub's API returns it. */
function ghRelease(/** @type {string} */ tag, /** @type {Record<string, any>} */ opts = {}) {
  const v = tag.replace(/^v/, '');
  const asset = (/** @type {string} */ name, /** @type {string | null} */ digest) => ({
    name,
    browser_download_url: dl(tag, name),
    digest,
  });
  return {
    tag_name: tag,
    draft: false,
    prerelease: false,
    published_at: '2026-10-21T19:34:12Z',
    assets: [
      asset(`Treq_${v}_x64.dmg`, `sha256:${'c'.repeat(64)}`),
      asset(`treq-cli-${v}-linux-x86_64.tar.gz`, `sha256:${X86}`),
      asset(`treq-cli-${v}-linux-aarch64.tar.gz`, `sha256:${ARM}`),
    ],
    ...opts,
  };
}

test('maps a release to its CLI tarballs with bare sha256 digests', () => {
  const out = toReleasesJson([ghRelease('v0.3.0')], '2026-11-15T00:00:00.000Z');
  expect(out).toEqual({
    schema: 1,
    generated_at: '2026-11-15T00:00:00.000Z',
    releases: [
      {
        version: '0.3.0',
        tag: 'v0.3.0',
        published_at: '2026-10-21T19:34:12Z',
        prerelease: false,
        assets: {
          'linux-x86_64': {url: dl('v0.3.0', 'treq-cli-0.3.0-linux-x86_64.tar.gz'), sha256: X86},
          'linux-aarch64': {url: dl('v0.3.0', 'treq-cli-0.3.0-linux-aarch64.tar.gz'), sha256: ARM},
        },
      },
    ],
  });
});

test('excludes drafts and keeps prereleases flagged', () => {
  const out = toReleasesJson(
    [ghRelease('v0.4.0', {draft: true, published_at: null}), ghRelease('nightly', {prerelease: true, assets: []})],
    'now',
  );
  expect(out.releases).toEqual([
    {version: 'nightly', tag: 'nightly', published_at: '2026-10-21T19:34:12Z', prerelease: true, assets: {}},
  ]);
});

test('a missing or malformed digest becomes null, and a missing tarball is left out', () => {
  const x86 = ghRelease('v0.2.0').assets[1];
  const arm = ghRelease('v0.1.0').assets[2];
  const out = toReleasesJson(
    [ghRelease('v0.2.0', {assets: [{...x86, digest: null}]}), ghRelease('v0.1.0', {assets: [{...arm, digest: 'md5:abc'}]})],
    'now',
  );
  expect(out.releases.map((r) => r.assets)).toEqual([
    {'linux-x86_64': {url: x86?.browser_download_url, sha256: null}},
    {'linux-aarch64': {url: arm?.browser_download_url, sha256: null}},
  ]);
});

test('orders releases newest first by publish date', () => {
  const out = toReleasesJson(
    [
      ghRelease('v0.2.0', {published_at: '2026-09-01T00:00:00Z'}),
      ghRelease('v0.10.0', {published_at: '2026-11-01T00:00:00Z'}),
      ghRelease('v0.9.0', {published_at: '2026-10-01T00:00:00Z'}),
    ],
    'now',
  );
  expect(out.releases.map((r) => r.tag)).toEqual(['v0.10.0', 'v0.9.0', 'v0.2.0']);
});

test('fetchAllReleases follows pages until a short page', async () => {
  /** @type {string[]} */
  const urls = [];
  const pages = [Array.from({length: 100}, (_, i) => ghRelease(`v0.0.${i}`)), [ghRelease('v0.0.100')]];
  /** @type {any} */
  const fakeFetch = async (/** @type {string} */ url) => {
    urls.push(url);
    return {ok: true, status: 200, json: async () => pages[urls.length - 1] ?? []};
  };
  const all = await fetchAllReleases(fakeFetch);
  expect(all).toHaveLength(101);
  expect(urls).toEqual([
    'https://api.github.com/repos/tznc/treq/releases?per_page=100&page=1',
    'https://api.github.com/repos/tznc/treq/releases?per_page=100&page=2',
  ]);
});

test('fetchAllReleases throws on a non-OK answer', async () => {
  /** @type {any} */
  const fakeFetch = async () => ({ok: false, status: 403, json: async () => ({})});
  await expect(fetchAllReleases(fakeFetch)).rejects.toThrow('403');
});
