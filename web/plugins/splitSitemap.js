// @ts-check

// Splits the one sitemap.xml Docusaurus writes into a sitemap per site
// section, and rewrites sitemap.xml as their index. Search Console and Bing
// report indexed vs submitted per sitemap, so each section's index coverage
// can be read on its own. Runs after `docusaurus build` (scripts/split-sitemap.mjs)
// because plugin postBuild hooks run in parallel with the sitemap plugin's.

const SECTIONS = ['pages', 'learn', 'docs', 'compare', 'tools', 'skill-authors', 'skills'];

/** @param {string} loc */
function sectionOf(loc) {
  const parts = new URL(loc).pathname.split('/').filter(Boolean);
  if (parts[0] === 'skills') return parts.length <= 2 ? 'skill-authors' : 'skills';
  return parts[0] && SECTIONS.includes(parts[0]) ? parts[0] : 'pages';
}

/**
 * @param {string} xml the sitemap.xml Docusaurus generated
 * @param {string} siteUrl e.g. https://treq.dev
 * @returns {{index: string, files: Record<string, string>}}
 */
function splitSitemap(xml, siteUrl) {
  if (/<sitemapindex[\s>]/.test(xml)) throw new Error('sitemap.xml is already a sitemap index');
  const open = xml.match(/^[\s\S]*?<urlset[^>]*>/)?.[0];
  if (!open) throw new Error('sitemap.xml has no <urlset>');

  /** @type {Record<string, string[]>} */
  const bySection = {};
  for (const url of xml.match(/<url>[\s\S]*?<\/url>/g) ?? []) {
    const loc = url.match(/<loc>([^<]*)<\/loc>/)?.[1];
    if (!loc) continue;
    const section = sectionOf(loc);
    bySection[section] = [...(bySection[section] ?? []), url];
  }

  /** @type {Record<string, string>} */
  const files = {};
  for (const section of SECTIONS) {
    const urls = bySection[section];
    if (urls) files[`sitemap-${section}.xml`] = `${open}${urls.join('')}</urlset>`;
  }
  const entries = Object.keys(files).map((name) => `<sitemap><loc>${siteUrl}/${name}</loc></sitemap>`);
  const index = `<?xml version="1.0" encoding="UTF-8"?><sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${entries.join('')}</sitemapindex>`;
  return {index, files};
}

module.exports = {splitSitemap};
