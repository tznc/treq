// @ts-check
const {keepInSitemap, createSitemapItems} = require('./sitemapSkills');

test.each(['/skills/', '/skills/expo/expo-dev-client/', 'https://treq.dev/skills/k-dense/rowan/'])(
  'keeps %s, the catalog or a skill page with search impressions',
  (url) => {
    expect(keepInSitemap(url)).toBe(true);
  },
);

test.each(['/skills/google/cloud/google-cloud-scc-query/', 'https://treq.dev/skills/coreyhaines/'])(
  'leaves out %s, a skill or author page with no search impressions',
  (url) => {
    expect(keepInSitemap(url)).toBe(false);
  },
);

test.each(['/', '/learn/answers/worktrees/remove-locked-worktree/', '/compare/orca-vs-superset/'])(
  'keeps %s, a page outside the skills catalog',
  (url) => {
    expect(keepInSitemap(url)).toBe(true);
  },
);

test('filters the default sitemap items', async () => {
  const items = [
    {url: 'https://treq.dev/learn/', lastmod: null, changefreq: null, priority: null},
    {url: 'https://treq.dev/skills/google/cloud/google-cloud-scc-query/', lastmod: null, changefreq: null, priority: null},
  ];
  // @ts-expect-error only the fields createSitemapItems reads
  const kept = await createSitemapItems({defaultCreateSitemapItems: async () => items});
  expect(kept.map((item) => item.url)).toEqual(['https://treq.dev/learn/']);
});
