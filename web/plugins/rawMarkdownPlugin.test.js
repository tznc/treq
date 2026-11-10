// @ts-check
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const {collectMarkdownPages, buildLlmsTxt, buildLlmsFullTxt} = require('./rawMarkdownPlugin');

function fixtureSite() {
  const siteDir = fs.mkdtempSync(path.join(os.tmpdir(), 'llms-'));
  const write = (rel, body) => {
    fs.mkdirSync(path.dirname(path.join(siteDir, rel)), {recursive: true});
    fs.writeFileSync(path.join(siteDir, rel), body);
  };
  write('docs/intro.md', '---\ndescription: Docs intro.\n---\n\n# Overview\n\nBody A');
  write('learn/git/rerere.mdx', '---\ndescription: Reuse resolutions.\n---\n\n# Git rerere\n\nBody B');
  write('learn/draft.md', '---\ndraft: true\n---\n\n# Draft\n');
  write('compare/treq-vs-orca.mdx', '# Treq vs Orca\n\nBody C');
  return siteDir;
}

test('collects title, description and url from docs, learn and compare', () => {
  const pages = collectMarkdownPages(fixtureSite());
  const rerere = pages.find((p) => p.outputPath === 'learn/git/rerere.md');
  assert.deepEqual(
    {title: rerere?.title, description: rerere?.description, section: rerere?.section},
    {title: 'Git rerere', description: 'Reuse resolutions.', section: 'learn'},
  );
  assert.ok(pages.some((p) => p.outputPath === 'compare/treq-vs-orca.md'));
  assert.ok(!pages.some((p) => p.outputPath === 'learn/draft.md'));
});

test('llms.txt links every page under its section', () => {
  const txt = buildLlmsTxt(collectMarkdownPages(fixtureSite()), {
    title: 'Treq',
    description: 'Tagline',
    url: 'https://treq.dev',
  });
  assert.match(txt, /^# Treq\n\n> Tagline\n/);
  assert.match(txt, /## Learn\n\n- \[Git rerere\]\(https:\/\/treq\.dev\/learn\/git\/rerere\.md\): Reuse resolutions\./);
  assert.match(txt, /## Compare\n\n- \[Treq vs Orca\]\(https:\/\/treq\.dev\/compare\/treq-vs-orca\.md\)\n/);
});

test('llms-full.txt inlines page content', () => {
  const txt = buildLlmsFullTxt(collectMarkdownPages(fixtureSite()), {
    title: 'Treq',
    description: 'Tagline',
    url: 'https://treq.dev',
  });
  for (const body of ['Body A', 'Body B', 'Body C']) assert.ok(txt.includes(body));
});
