// @ts-check
const {splitSitemap} = require('./splitSitemap');

const URLSET_OPEN = '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">';

/** @param {string[]} paths */
function sitemap(paths) {
  const urls = paths.map((p) => `<url><loc>https://treq.dev${p}</loc><changefreq>weekly</changefreq></url>`);
  return `<?xml version="1.0" encoding="UTF-8"?>${URLSET_OPEN}${urls.join('')}</urlset>`;
}

/** @param {string} xml */
const locs = (xml) => [...xml.matchAll(/<loc>([^<]*)<\/loc>/g)].map((m) => m[1]);

test('puts each section in its own sitemap, skill pages apart from the skills hub and author pages', () => {
  const {files} = splitSitemap(
    sitemap([
      '/',
      '/pricing/',
      '/learn/concepts/git/zdiff3/',
      '/docs/',
      '/compare/treq-vs-orca/',
      '/tools/dag-visualizer/',
      '/skills/',
      '/skills/mattpocock/',
      '/skills/mattpocock/engineering/wizard/',
    ]),
    'https://treq.dev',
  );
  expect(Object.fromEntries(Object.entries(files).map(([name, xml]) => [name, locs(xml)]))).toEqual({
    'sitemap-pages.xml': ['https://treq.dev/', 'https://treq.dev/pricing/'],
    'sitemap-learn.xml': ['https://treq.dev/learn/concepts/git/zdiff3/'],
    'sitemap-docs.xml': ['https://treq.dev/docs/'],
    'sitemap-compare.xml': ['https://treq.dev/compare/treq-vs-orca/'],
    'sitemap-tools.xml': ['https://treq.dev/tools/dag-visualizer/'],
    'sitemap-skill-authors.xml': ['https://treq.dev/skills/', 'https://treq.dev/skills/mattpocock/'],
    'sitemap-skills.xml': ['https://treq.dev/skills/mattpocock/engineering/wizard/'],
  });
});

test('keeps each url entry and the urlset namespaces as generated', () => {
  const {files} = splitSitemap(sitemap(['/learn/']), 'https://treq.dev');
  expect(files['sitemap-learn.xml']).toBe(sitemap(['/learn/']));
});

test('turns sitemap.xml into an index of the non-empty section sitemaps', () => {
  const {index} = splitSitemap(sitemap(['/', '/learn/']), 'https://treq.dev');
  expect(index).toMatch(/^<\?xml version="1\.0" encoding="UTF-8"\?><sitemapindex xmlns="http:\/\/www\.sitemaps\.org\/schemas\/sitemap\/0\.9">/);
  expect(locs(index)).toEqual(['https://treq.dev/sitemap-pages.xml', 'https://treq.dev/sitemap-learn.xml']);
});

test('refuses a sitemap that is already an index, so a second run fails loudly', () => {
  const {index} = splitSitemap(sitemap(['/']), 'https://treq.dev');
  expect(() => splitSitemap(index, 'https://treq.dev')).toThrow('already a sitemap index');
});
