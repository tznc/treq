// @ts-check
const {lockProblems, nextLock, sourceCacheKey} = require('./skillsLock');

/** @param {string} c */
const sha = (c) => c.repeat(40);
const sources = [
  {id: 'a', org: 'o', repo: 'a', branch: 'main', skillsRoot: 'skills', excludeCategories: []},
  {id: 'b', org: 'o', repo: 'b', branch: 'main', skillsRoot: '', excludeCategories: []},
];

describe('lockProblems', () => {
  test('passes a lock that pins every source to a full commit SHA', () => {
    expect(lockProblems(sources, {a: {commit: sha('1'), stars: 3}, b: {commit: sha('2'), stars: null}})).toEqual([]);
  });

  test('names sources that are missing or not pinned to a full SHA', () => {
    expect(lockProblems(sources, {a: {commit: 'main', stars: 1}})).toEqual([
      'a: commit "main" is not a 40-character SHA',
      'b: missing from skills.lock.json',
    ]);
  });

  test('names lock entries for sources that no longer exist', () => {
    const lock = {a: {commit: sha('1'), stars: 1}, b: {commit: sha('2'), stars: 1}, gone: {commit: sha('3'), stars: 1}};
    expect(lockProblems(sources, lock)).toEqual(['gone: not a source in fetch-skills-catalog.mjs']);
  });
});

describe('nextLock', () => {
  test('pins each source to its branch head, in source order', () => {
    const lock = nextLock(sources, {}, {b: sha('2'), a: sha('1')}, {a: 10, b: 20});
    expect(Object.keys(lock)).toEqual(['a', 'b']);
    expect(lock).toEqual({a: {commit: sha('1'), stars: 10}, b: {commit: sha('2'), stars: 20}});
  });

  test('keeps the previous star count when the API gives none', () => {
    expect(nextLock([sources[0]], {a: {commit: sha('0'), stars: 7}}, {a: sha('1')}, {a: null}).a.stars).toBe(7);
  });

  test('drops entries for removed sources', () => {
    expect(nextLock([sources[0]], {gone: {commit: sha('9'), stars: 1}}, {a: sha('1')}, {})).toEqual({
      a: {commit: sha('1'), stars: null},
    });
  });
});

describe('sourceCacheKey', () => {
  const key = sourceCacheKey(sources[0], sha('1'), 'code');

  test('is stable and starts with the source id', () => {
    expect(sourceCacheKey(sources[0], sha('1'), 'code')).toBe(key);
    expect(key).toMatch(/^a-[0-9a-f]{16}$/);
  });

  test('changes with the commit, the source config or the generator code', () => {
    expect(sourceCacheKey(sources[0], sha('2'), 'code')).not.toBe(key);
    expect(sourceCacheKey({...sources[0], skillsRoot: 'other'}, sha('1'), 'code')).not.toBe(key);
    expect(sourceCacheKey(sources[0], sha('1'), 'code2')).not.toBe(key);
  });
});
