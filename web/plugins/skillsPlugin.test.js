// @ts-check
const {buildAuthorPages, skillPageTitle, skillPageProps} = require('./skillsPlugin');

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

describe('skillPageProps', () => {
  const mit = {...catalog.skills[0], license: 'MIT', path: 'skills/engineering/wizard'};
  const pages = {
    [mit.id]: {body: '# Wizard', licenseUrl: 'https://github.com/mattpocock/skills/blob/main/LICENSE', manualOnly: true},
  };

  test('passes the SKILL.md body, author and generated content for a redistributable license', () => {
    const props = skillPageProps(mit, {...catalog, skills: [mit, catalog.skills[1]]}, pages);
    expect(props.title).toBe('wizard skill by Matt Pocock');
    expect(props.author).toBe('Matt Pocock');
    expect(props.authorRoute).toBe('/skills/mattpocock');
    expect(props.body).toBe('# Wizard');
    expect(props.licenseUrl).toBe('https://github.com/mattpocock/skills/blob/main/LICENSE');
    expect(props.manualOnly).toBe(true);
    expect(props.install?.folder).toBe('wizard');
    expect(props.related.map((s) => s.name)).toEqual(['chief-of-staff']);
    expect(props.metaDescription).toBe('Wizard.');
    expect(props.skill.files).toEqual(mit.files);
  });

  test('withholds the body when the license does not permit redistribution', () => {
    expect(skillPageProps({...mit, license: 'Custom'}, catalog, pages).body).toBeNull();
  });

  test('writes a meta description when the skill has none', () => {
    const props = skillPageProps(catalog.skills[1], catalog, {});
    expect(props.metaDescription).toBe(
      'chief-of-staff is an agent skill by Matt Pocock. Install it in Claude Code, Codex or Treq.',
    );
    expect(props.body).toBeNull();
    expect(props.manualOnly).toBe(false);
  });
});

test('skill page title names the author', () => {
  expect(skillPageTitle(catalog.skills[0], catalog.sources)).toBe('wizard skill by Matt Pocock');
  expect(skillPageTitle({...catalog.skills[0], source: 'unknown'}, catalog.sources)).toBe('wizard skill by unknown');
});
