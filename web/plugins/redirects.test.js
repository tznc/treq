// @ts-check
// static/_redirects is Cloudflare Pages' redirect table. A redirect must point
// at a page that exists, and must not shadow a page that still exists.
const fs = require('fs');
const path = require('path');
const {collectMarkdownPages} = require('./rawMarkdownPlugin');

const siteDir = path.join(__dirname, '..');
const routes = new Set(
  collectMarkdownPages(siteDir).map((p) => `/${p.outputPath.replace(/(\/index)?\.md$/, '')}/`),
);
const rules = fs
  .readFileSync(path.join(siteDir, 'static/_redirects'), 'utf8')
  .split('\n')
  .filter((line) => line.trim() && !line.startsWith('#'))
  .map((line) => line.trim().split(/\s+/));

test('moved worktree vs clone guide redirects to the concept page', () => {
  expect(rules).toContainEqual(['/learn/how-to/git-worktrees-vs-clones-for-agents/', '/learn/concepts/git/git-worktrees-vs-clones/', '301']);
});

test.each(rules)('%s -> %s (%s) targets a live page and shadows none', (from, to, status) => {
  expect(status).toBe('301');
  expect(routes.has(to)).toBe(true);
  expect(routes.has(from.endsWith('/') ? from : `${from}/`)).toBe(false);
});
