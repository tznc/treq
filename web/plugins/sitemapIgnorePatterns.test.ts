import {createMatcher} from '@docusaurus/utils';
import config from '../docusaurus.config';

// Routes robots.txt disallows, and search results, which are not pages to index.
const isIgnored = createMatcher(
  ((config.presets?.[0] as [string, {sitemap: {ignorePatterns: string[]}}])[1]).sitemap.ignorePatterns,
);

test.each(['/dashboard/', '/login/', '/sign-in/', '/auth/callback/', '/integrations/linear/', '/search/'])(
  'leaves %s out of the sitemap',
  (route) => {
    expect(isIgnored(route)).toBe(true);
  },
);

test('keeps content pages in the sitemap', () => {
  expect(isIgnored('/learn/concepts/git/zdiff3/')).toBe(false);
});

test('lists only skill pages with search impressions', () => {
  const sitemap = (config.presets?.[0] as [string, {sitemap: {createSitemapItems?: unknown}}])[1].sitemap;
  expect(sitemap.createSitemapItems).toBe(require('./sitemapSkills').createSitemapItems);
});
