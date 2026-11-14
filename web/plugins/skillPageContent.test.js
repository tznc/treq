// @ts-check
const {
  canRedistribute,
  truncateDescription,
  relatedSkills,
  installCommands,
  triggerClause,
} = require('./skillPageContent');

describe('canRedistribute', () => {
  test.each(['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Unlicense'])('allows %s', (license) => {
    expect(canRedistribute(license)).toBe(true);
  });

  test.each(['Proprietary', 'Custom', null, undefined, '', 'GPL-3.0'])('refuses %p', (license) => {
    expect(canRedistribute(license)).toBe(false);
  });
});

describe('truncateDescription', () => {
  test('leaves short text alone but collapses whitespace', () => {
    expect(truncateDescription('Grill  me\n about a plan.')).toBe('Grill me about a plan.');
  });

  test('returns an empty string for missing text', () => {
    expect(truncateDescription(null)).toBe('');
  });

  test('cuts at the last sentence end when it keeps most of the text', () => {
    const first = 'Configure Azure API Management as an AI Gateway for AI models, MCP tools, and agents.';
    const text = `${first} WHEN: semantic caching, token limit, content safety, load balancing, rate limiting and more words here.`;
    expect(truncateDescription(text)).toBe(first);
  });

  test('otherwise cuts at a word boundary, drops trailing punctuation and adds an ellipsis', () => {
    const text = `${'word '.repeat(30)}tail, end of a very long sentence with no full stop anywhere in it at all`;
    const out = truncateDescription(text, 160);
    expect(out.length).toBeLessThanOrEqual(160);
    expect(out.endsWith('…')).toBe(true);
    expect(out).not.toMatch(/[,\s]…$/);
    expect(text.startsWith(out.slice(0, -1))).toBe(true);
    expect(text[out.length - 1]).toMatch(/[\s,]/);
  });

  test('honours a custom limit', () => {
    expect(truncateDescription('alpha beta gamma delta', 12)).toBe('alpha beta…');
  });
});

describe('relatedSkills', () => {
  const sources = [
    {id: 'matt', name: 'Matt Pocock', url: 'https://github.com/mattpocock/skills'},
    {id: 'other', name: 'Other', url: 'https://github.com/o/r'},
  ];
  /** @type {(id: string, source: string, category: string | null) => any} */
  const skill = (id, source, category) => ({
    id,
    name: id,
    description: `${id} does things.`,
    source,
    category,
    route: `/skills/${id}`,
  });
  const skills = [
    skill('zeta', 'matt', 'Engineering'),
    skill('self', 'matt', 'Engineering'),
    skill('alpha', 'matt', 'Engineering'),
    skill('beta', 'matt', 'Productivity'),
    skill('gamma', 'other', 'Engineering'),
    skill('delta', 'other', 'Writing'),
  ];

  test('orders same author and category, then same author, then same category elsewhere', () => {
    const related = relatedSkills(skills[1], skills, sources);
    expect(related.map((s) => s.name)).toEqual(['alpha', 'zeta', 'beta', 'gamma']);
    expect(related[3]).toEqual({
      name: 'gamma',
      route: '/skills/gamma',
      author: 'Other',
      description: 'gamma does things.',
    });
  });

  test('respects the limit', () => {
    expect(relatedSkills(skills[1], skills, sources, 2).map((s) => s.name)).toEqual(['alpha', 'zeta']);
  });

  test('does not match other authors on a missing category', () => {
    const lone = skill('lone', 'other', null);
    expect(relatedSkills(lone, [...skills, lone, skill('nil', 'matt', null)], sources).map((s) => s.name)).toEqual([
      'delta',
      'gamma',
    ]);
  });
});

describe('installCommands', () => {
  const source = {id: 'mattpocock', name: 'Matt Pocock', url: 'https://github.com/mattpocock/skills'};
  const skill = {name: 'chief-of-staff', path: 'skills/in-progress/chief-of-staff'};

  test('clones the repo once and copies the skill folder into each agent skills directory', () => {
    const {folder, claudeCode, codex} = installCommands(skill, source);
    expect(folder).toBe('chief-of-staff');
    expect(claudeCode).toBe(
      [
        'git clone --depth 1 https://github.com/mattpocock/skills.git /tmp/mattpocock-skills',
        'mkdir -p .claude/skills',
        'cp -r /tmp/mattpocock-skills/skills/in-progress/chief-of-staff .claude/skills/',
      ].join('\n'),
    );
    expect(codex).toBe(
      [
        'git clone --depth 1 https://github.com/mattpocock/skills.git /tmp/mattpocock-skills',
        'mkdir -p .agents/skills',
        'cp -r /tmp/mattpocock-skills/skills/in-progress/chief-of-staff .agents/skills/',
      ].join('\n'),
    );
  });

  test('uses the folder name for skills at the repo root', () => {
    expect(installCommands({path: 'prisma-cli'}, {...source, url: 'https://github.com/prisma/skills'}).folder).toBe(
      'prisma-cli',
    );
  });
});

describe('triggerClause', () => {
  test('finds a "Use when" sentence', () => {
    expect(
      triggerClause('Write release notes. Use when the user asks for a changelog or release notes. Keep it short.'),
    ).toBe('Use when the user asks for a changelog or release notes.');
  });

  test('finds a WHEN: list', () => {
    expect(triggerClause('Azure VM router. WHEN: create VM, recommend VM size, compare VM pricing.')).toBe(
      'WHEN: create VM, recommend VM size, compare VM pricing.',
    );
  });

  test('finds "Trigger on"', () => {
    expect(triggerClause('Guide. Trigger on: "how do I", "teach me".')).toBe('Trigger on: "how do I", "teach me".');
  });

  test('returns null when the description names no trigger', () => {
    expect(triggerClause('A relentless interview to sharpen a plan or design.')).toBeNull();
    expect(triggerClause(null)).toBeNull();
  });

  test('trims a long clause cleanly', () => {
    const out = triggerClause(`Use this skill when ${'the user does a thing, '.repeat(20)}finally.`);
    expect(out?.length).toBeLessThanOrEqual(240);
    expect(out?.startsWith('Use this skill when the user')).toBe(true);
    expect(out?.endsWith('…')).toBe(true);
  });
});
