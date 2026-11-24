// @ts-check
// Search Console had indexed 88 of ~850 skill pages and never seen 494 (Nov 2026), while
// Learn and Docs pages sat in "Discovered - currently not indexed". The sitemap lists
// /skills/ and only the skill pages that earned Search Console impressions
// (src/data/skills-sitemap.json); the rest stay live and reachable through links.
/** @type {Set<string>} */
const withImpressions = new Set(require('../src/data/skills-sitemap.json'));

/** @param {string} url an absolute URL or a path */
function keepInSitemap(url) {
  const {pathname} = new URL(url, 'https://treq.dev');
  if (!pathname.startsWith('/skills/') || pathname === '/skills/') return true;
  return withImpressions.has(pathname);
}

/** @type {NonNullable<import('@docusaurus/plugin-sitemap').PluginOptions['createSitemapItems']>} */
async function createSitemapItems({defaultCreateSitemapItems, ...params}) {
  const items = await defaultCreateSitemapItems(params);
  return items.filter((item) => keepInSitemap(item.url));
}

module.exports = {keepInSitemap, createSitemapItems};
