// @ts-check
const fs = require('fs');
const os = require('os');
const path = require('path');
const {buildAuthorPages, loadAuthorWriteups, skillPageTitle, skillPageProps} = require('./skillsPlugin');

const wizard = {id: 'a', name: 'wizard', description: 'Wizard.', source: 'mattpocock', category: 'Engineering', route: '/skills/mattpocock/engineering/wizard', files: [{}]};
const chief = {id: 'b', name: 'chief-of-staff', description: null, source: 'mattpocock', category: 'In Progress', route: '/skills/mattpocock/in-progress/chief-of-staff', files: [{}]};
const catalog = {
  sources: [
    {id: 'mattpocock', name: 'Matt Pocock', url: 'https://github.com/mattpocock/skills', stars: 10},
    {id: 'empty', name: 'Empty', url: 'https://github.com/x/y', stars: 0},
  ],
  skills: [wizard, chief],
};

test('one author page per source with skills, sorted by name and without file lists', () => {
  const pages = buildAuthorPages(catalog);
  expect(pages.length).toBe(1);
  const [page] = pages;
  expect(page?.route).toBe('/skills/mattpocock');
  expect(page?.source).toEqual(catalog.sources[0]);
  expect(page?.skills.map((s) => s.name)).toEqual(['chief-of-staff', 'wizard']);
  expect(page?.skills.every((s) => !('files' in s))).toBeTruthy();
  expect(page?.writeup).toBeNull();
});

test('an author page carries its write-up when one exists', () => {
  const writeup = {description: 'Matt Pocock skills.', body: '## History\n\nText.'};
  const [page] = buildAuthorPages(catalog, {mattpocock: writeup});
  expect(page?.writeup).toEqual(writeup);
});

describe('loadAuthorWriteups', () => {
  test('reads each <id>.md into its description and body', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'writeups-'));
    fs.writeFileSync(path.join(dir, 'mattpocock.md'), '---\ndescription: "Matt Pocock skills."\n---\n\n## History\n\nText.\n');
    fs.writeFileSync(path.join(dir, 'notes.txt'), 'ignored');
    expect(loadAuthorWriteups(dir)).toEqual({
      mattpocock: {description: 'Matt Pocock skills.', body: '## History\n\nText.'},
    });
  });

  test('returns no write-ups when the folder is missing', () => {
    expect(loadAuthorWriteups(path.join(os.tmpdir(), 'no-such-writeups'))).toEqual({});
  });
});

describe('skillPageProps', () => {
  const mit = {...wizard, license: 'MIT', path: 'skills/engineering/wizard'};
  const pages = {
    [mit.id]: {body: '# Wizard', licenseUrl: 'https://github.com/mattpocock/skills/blob/main/LICENSE', manualOnly: true},
  };

  test('passes the SKILL.md body, author and generated content for a redistributable license', () => {
    const props = skillPageProps(mit, {...catalog, skills: [mit, chief]}, pages);
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
    const props = skillPageProps(chief, catalog, {});
    expect(props.metaDescription).toBe(
      'chief-of-staff is an agent skill by Matt Pocock. Install it in Claude Code, Codex or Treq.',
    );
    expect(props.body).toBeNull();
    expect(props.manualOnly).toBe(false);
  });
});

test('skill page title names the author', () => {
  expect(skillPageTitle(wizard, catalog.sources)).toBe('wizard skill by Matt Pocock');
  expect(skillPageTitle({...wizard, source: 'unknown'}, catalog.sources)).toBe('wizard skill by unknown');
});

// Write-ups live in web/skill-authors, one per catalog source. A file whose
// name is not a source id would never render, so it fails here instead.
test('every committed author write-up names a source and has every section', () => {
  const writeups = loadAuthorWriteups(path.join(__dirname, '..', 'skill-authors'));
  const lock = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'skills.lock.json'), 'utf8'));
  const sections = ['History', 'Depth', 'Tooling', 'Community engagement', 'Utility for target workloads'];
  const problems = Object.entries(writeups).flatMap(([id, {description, body}]) => [
    ...(id in lock ? [] : [`${id}: not a source in skills.lock.json`]),
    ...(description && description.length <= 160 ? [] : [`${id}: description missing or over 160 characters`]),
    ...(JSON.stringify(body.match(/^## .+$/gm)) === JSON.stringify(sections.map((h) => `## ${h}`))
      ? []
      : [`${id}: sections must be ${sections.join(', ')}`]),
  ]);
  expect(problems).toEqual([]);
});
