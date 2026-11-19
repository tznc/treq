import {createMatcher} from '@docusaurus/utils';
import config from '../docusaurus.config';

// Routes robots.txt disallows; listing them in the sitemap sends Google mixed signals.
const isIgnored = createMatcher(
  ((config.presets?.[0] as [string, {sitemap: {ignorePatterns: string[]}}])[1]).sitemap.ignorePatterns,
);

test.each(['/dashboard/', '/login/', '/sign-in/', '/auth/callback/', '/integrations/linear/'])(
  'leaves %s out of the sitemap',
  (route) => {
    expect(isIgnored(route)).toBe(true);
  },
);

test('keeps content pages in the sitemap', () => {
  expect(isIgnored('/learn/concepts/git/zdiff3/')).toBe(false);
});
