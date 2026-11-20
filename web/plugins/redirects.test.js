// @ts-check
// static/_redirects is Cloudflare Pages' redirect table. A 301 must point at a
// page that exists, and must not shadow a page that still exists. 200 rules are
// rewrites to app pages, not moved content, so they are not checked here.
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
const moved = rules.filter(([, , status]) => status === '301');

test('moved worktree vs clone guide redirects to the concept page', () => {
  expect(rules).toContainEqual(['/learn/how-to/git-worktrees-vs-clones-for-agents/', '/learn/concepts/git/git-worktrees-vs-clones/', '301']);
});

test.each(moved)('%s -> %s (%s) targets a live page and shadows none', (from, to) => {
  expect(routes.has(to)).toBe(true);
  expect(routes.has(from.endsWith('/') ? from : `${from}/`)).toBe(false);
});
