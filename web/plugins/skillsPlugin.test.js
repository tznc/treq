// @ts-check
const test = require('node:test');
const assert = require('node:assert/strict');
const {buildAuthorPages, skillPageTitle} = require('./skillsPlugin');

const catalog = {
  sources: [
    {id: 'mattpocock', name: 'Matt Pocock', url: 'https://github.com/mattpocock/skills', stars: 10},
    {id: 'empty', name: 'Empty', url: 'https://github.com/x/y', stars: 0},
  ],
  skills: [
    {id: 'a', name: 'wizard', description: 'Wizard.', source: 'mattpocock', category: 'Engineering', route: '/skills/mattpocock/engineering/wizard', files: [{}]},
    {id: 'b', name: 'chief-of-staff', description: null, source: 'mattpocock', category: 'In Progress', route: '/skills/mattpocock/in-progress/chief-of-staff', files: [{}]},
  ],
};

test('one author page per source with skills, sorted by name and without file lists', () => {
  const pages = buildAuthorPages(catalog);
  assert.equal(pages.length, 1);
  const [page] = pages;
  assert.equal(page.route, '/skills/mattpocock');
  assert.deepEqual(page.source, catalog.sources[0]);
  assert.deepEqual(
    page.skills.map((s) => s.name),
    ['chief-of-staff', 'wizard'],
  );
  assert.ok(page.skills.every((s) => !('files' in s)));
});

test('skill page title names the author', () => {
  assert.equal(skillPageTitle(catalog.skills[0], catalog.sources), 'wizard skill by Matt Pocock');
  assert.equal(skillPageTitle({...catalog.skills[0], source: 'unknown'}, catalog.sources), 'wizard skill by unknown');
});
