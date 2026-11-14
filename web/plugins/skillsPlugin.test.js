// @ts-check
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
  expect(pages.length).toBe(1);
  const [page] = pages;
  expect(page.route).toBe('/skills/mattpocock');
  expect(page.source).toEqual(catalog.sources[0]);
  expect(page.skills.map((s) => s.name)).toEqual(['chief-of-staff', 'wizard']);
  expect(page.skills.every((s) => !('files' in s))).toBeTruthy();
});

test('skill page title names the author', () => {
  expect(skillPageTitle(catalog.skills[0], catalog.sources)).toBe('wizard skill by Matt Pocock');
  expect(skillPageTitle({...catalog.skills[0], source: 'unknown'}, catalog.sources)).toBe('wizard skill by unknown');
});
