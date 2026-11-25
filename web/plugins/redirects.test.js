// @ts-check
// static/_redirects is Cloudflare Pages' redirect table. A 301 must point at a
// page that exists, and must not shadow a page that still exists. 200 rules are
// rewrites to app pages, not moved content, so they are not checked here.
const fs = require('fs');
const path = require('path');
const {collectMarkdownPages} = require('./rawMarkdownPlugin');
const {buildAuthorPages} = require('./skillsPlugin');

const siteDir = path.join(__dirname, '..');
const routes = new Set(
  collectMarkdownPages(siteDir).map((p) => `/${p.outputPath.replace(/(\/index)?\.md$/, '')}/`),
);
// Skill and author pages come from the generated catalog (npm run build:skills-catalog).
const catalog = require('../src/data/skills.json');
for (const {route} of [...catalog.skills, ...buildAuthorPages(catalog)]) routes.add(`${route}/`);
// Tools are React pages (src/pages/tools/<name>.tsx); files starting with _ are not routes.
for (const file of fs.readdirSync(path.join(siteDir, 'src/pages/tools'))) {
  if (file.endsWith('.tsx') && !file.startsWith('_')) routes.add(`/tools/${file === 'index.tsx' ? '' : `${file.slice(0, -4)}/`}`);
}
const rules = fs
  .readFileSync(path.join(siteDir, 'static/_redirects'), 'utf8')
  .split('\n')
  .filter((line) => line.trim() && !line.startsWith('#'))
  .map((line) => line.trim().split(/\s+/));
const moved = rules.filter(([, , status]) => status === '301');

// Cloudflare Pages allows 100 dynamic rules (splats or placeholders) and wants
// static rules first. In production, only the first 100 rules applied while a
// dynamic rule sat at the top of the file.
/** @param {string[]} rule */
const isDynamic = ([from]) => /[*:]/.test(from ?? '');

test('static rules come before dynamic ones', () => {
  const firstDynamic = rules.findIndex(isDynamic);
  expect(firstDynamic === -1 || rules.slice(firstDynamic).every(isDynamic)).toBe(true);
});

test('stays within the Cloudflare Pages rule limits', () => {
  expect(rules.filter(isDynamic).length).toBeLessThanOrEqual(100);
  expect(rules.filter((rule) => !isDynamic(rule)).length).toBeLessThanOrEqual(2000);
});

test('moved worktree vs clone guide redirects to the concept page', () => {
  expect(rules).toContainEqual(['/learn/how-to/git-worktrees-vs-clones-for-agents/', '/learn/concepts/git/git-worktrees-vs-clones/', '301']);
});

test.each(moved)('%s -> %s (%s) targets a live page and shadows none', (from, to) => {
  expect(routes.has(to)).toBe(true);
  expect(routes.has(from.endsWith('/') ? from : `${from}/`)).toBe(false);
});

// Pages and skills that moved or were deleted. Search Console reported each as
// a 404, so each needs a 301 with and without the trailing slash.
const retired = [
  '/skills/mattpocock/engineering/resolving-merge-conflicts/',
  '/skills/mattpocock/in-progress/batch-grill-me/',
  '/skills/mattpocock/in-progress/implement-spec/',
  '/skills/mattpocock/in-progress/pr/',
  '/skills/mattpocock/in-progress/retro/',
  '/skills/mattpocock/in-progress/to-questionnaire/',
  '/skills/mattpocock/in-progress/wizard/',
  '/skills/mattpocock/personal/edit-article/',
  '/skills/mattpocock/personal/obsidian-vault/',
  '/skills/mattpocock/productivity/writing-great-skills/',
  '/skills/microsoft/azure-cost/',
  '/docs/cli/',
  '/docs/contributing/',
  '/docs/keyboard-shortcuts/',
  '/docs/concepts/reviews/',
  '/docs/concepts/trello-and-jira-integrations/',
  '/docs/features/code-review/',
  '/docs/features/commit-management/',
  '/docs/features/implementation-plans/',
  '/docs/features/reviews/',
  '/docs/features/terminal-sessions/',
  '/docs/features/workspaces/',
  '/docs/how-to/scheduling-workspaces/',
  '/docs/how-to/using-the-merge-queue/',
  '/docs/guides/getting-started/installation/',
  '/docs/guides/getting-started/your-first-workspace/',
  '/docs/guides/common-tasks/discarding-changes/',
  '/docs/guides/common-tasks/moving-files-between-workspaces/',
  '/docs/guides/common-tasks/pushing-to-remote/',
  '/docs/guides/tips-and-tricks/customizing-settings/',
  '/docs/guides/core-workflows/code-review-workflow/',
  '/docs/guides/core-workflows/committing-changes/',
  '/docs/guides/core-workflows/creating-terminal-sessions/',
  '/docs/guides/core-workflows/executing-implementation-plans/',
  '/docs/guides/core-workflows/managing-workspaces/',
  '/docs/guides/core-workflows/merging-workspaces/',
  '/docs/guides/core-workflows/staging-and-committing/',
  '/docs/guides/core-workflows/using-treq-with-git-repo/',
  '/learn/troubleshooting/',
  '/learn/how-to/customizing-settings/',
  '/learn/how-to/discarding-changes/',
  '/learn/how-to/moving-files-between-workspaces/',
  '/learn/how-to/pushing-to-remote/',
  '/learn/tutorials/code-review-workflow/',
  '/learn/tutorials/committing-changes/',
  '/learn/tutorials/creating-terminal-sessions/',
  '/learn/tutorials/managing-workspaces/',
  '/learn/tutorials/merging-workspaces/',
  '/learn/tutorials/using-treq-with-git-repo/',
  '/learn/workflows/stacks/',
  '/learn/workflows/stacks/elixir-ai/',
  '/learn/workflows/stacks/react-development/',
  '/learn/workflows/stacks/rust-refactoring/',
  '/learn/workflows/stacks/tauri-development/',
  '/tools/dag-visualizer/',
];

test.each(retired)('%s redirects with and without the trailing slash', (url) => {
  const from = new Set(moved.map(([source]) => source));
  expect(from.has(url)).toBe(true);
  expect(from.has(url.slice(0, -1))).toBe(true);
});
