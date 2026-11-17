// @ts-check
const {sourceRoots, discoverSkills, licenseDirs, detectLicense} = require('./skillDiscovery');

describe('sourceRoots', () => {
  test('uses skillsRoots when a source lists several', () => {
    expect(sourceRoots({skillsRoots: ['skills', 'plugins/*/skills']})).toEqual(['skills', 'plugins/*/skills']);
  });

  test('falls back to the single skillsRoot', () => {
    expect(sourceRoots({skillsRoot: 'skills'})).toEqual(['skills']);
    expect(sourceRoots({skillsRoot: ''})).toEqual(['']);
  });
});

describe('discoverSkills', () => {
  /** @param {ReturnType<typeof discoverSkills>} result */
  const relDirs = (result) => result.skills.map((s) => s.relDir);

  test('keeps the skill folders under a root, with their repo path', () => {
    const result = discoverSkills(['skills/pdf/SKILL.md', 'docs/SKILL.md', 'other/x/SKILL.md'], ['skills']);
    expect(result.skills).toEqual([{file: 'skills/pdf/SKILL.md', dir: 'skills/pdf', relDir: 'pdf'}]);
  });

  test('keeps nested folders, whose first segment is the category', () => {
    expect(relDirs(discoverSkills(['skills/cloud/gcloud/SKILL.md'], ['skills']))).toEqual(['cloud/gcloud']);
  });

  test('never treats a SKILL.md in the root folder itself as a skill', () => {
    // A repo-root SKILL.md would get the route /skills/<author>/, the author page.
    expect(relDirs(discoverSkills(['SKILL.md', 'a/SKILL.md'], ['']))).toEqual(['a']);
    expect(relDirs(discoverSkills(['skills/SKILL.md', 'skills/a/SKILL.md'], ['skills']))).toEqual(['a']);
  });

  test('a * segment matches one folder and stays in the path, so plugins do not collide', () => {
    const files = [
      'plugins/discord/skills/access/SKILL.md',
      'plugins/telegram/skills/access/SKILL.md',
      'plugins/telegram/README/SKILL.md',
    ];
    expect(relDirs(discoverSkills(files, ['plugins/*/skills']))).toEqual(['discord/access', 'telegram/access']);
  });

  test('a * segment skips hidden folders', () => {
    expect(relDirs(discoverSkills(['plugins/.cursor/skills/a/SKILL.md'], ['plugins/*/skills']))).toEqual([]);
  });

  test('each SKILL.md belongs to the first root that holds it', () => {
    const files = ['openclaw/skills/retro/SKILL.md', 'ship/SKILL.md'];
    expect(relDirs(discoverSkills(files, ['openclaw/skills', '']))).toEqual(['retro', 'ship']);
  });

  test('when two roots hold a skill at the same path, the first root wins and the other is skipped', () => {
    const files = ['plugins/neon/skills/neon/SKILL.md', 'skills/neon/SKILL.md', 'skills/auth/SKILL.md'];
    const result = discoverSkills(files, ['skills', 'plugins/neon/skills']);
    expect(result.skills.map((s) => s.dir)).toEqual(['skills/auth', 'skills/neon']);
    expect(result.skipped).toEqual([{file: 'plugins/neon/skills/neon/SKILL.md', relDir: 'neon'}]);
  });

  test('a later glob root cannot shadow an earlier root either', () => {
    const files = ['skills/terraform/plan/SKILL.md', 'plugins/terraform/skills/plan/SKILL.md'];
    const result = discoverSkills(files, ['skills', 'plugins/*/skills']);
    expect(result.skills.map((s) => s.dir)).toEqual(['skills/terraform/plan']);
    expect(result.skipped.map((s) => s.relDir)).toEqual(['terraform/plan']);
  });

  test('accepts Windows separators', () => {
    expect(relDirs(discoverSkills(['skills\\pdf\\SKILL.md'], ['skills']))).toEqual(['pdf']);
  });
});

describe('licenseDirs', () => {
  test('lists the skill folder and each parent up to the repo root, nearest first', () => {
    expect(licenseDirs('plugins/security/skills/audit')).toEqual([
      'plugins/security/skills/audit',
      'plugins/security/skills',
      'plugins/security',
      'plugins',
      '',
    ]);
  });
});

describe('detectLicense', () => {
  test('reads MPL-2.0, whose text names the GPL as a secondary license', () => {
    const mpl = `Mozilla Public License Version 2.0
==================================
1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU Lesser
    General Public License, Version 2.1, the GNU Affero General Public
    License, Version 3.0, or any later versions of those licenses.`;
    expect(detectLicense(mpl)).toBe('MPL-2.0');
  });

  test('reads common licenses and marks unknown text as Custom', () => {
    expect(detectLicense('MIT License\n\nCopyright (c) 2026')).toBe('MIT');
    expect(detectLicense('Apache License\n Version 2.0, January 2004')).toBe('Apache-2.0');
    expect(detectLicense('GNU GENERAL PUBLIC LICENSE\n Version 3, 29 June 2007')).toBe('GPL-3.0');
    expect(detectLicense('Copyright (c) 2026 Anthropic, PBC. All rights reserved. ... is proprietary')).toBe('Proprietary');
    expect(detectLicense('Do what you like.')).toBe('Custom');
  });
});
