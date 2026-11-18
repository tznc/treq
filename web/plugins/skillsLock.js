// @ts-check
// Pure helpers for web/skills.lock.json, which pins each skills catalog source
// (see scripts/fetch-skills-catalog.mjs) to a commit and records its stars.
// The catalog is generated from the lock at build time instead of being
// committed; only the lock changes when sources update.
const {createHash} = require('crypto');

/** @typedef {{commit: string, stars: number | null}} LockEntry */
/** @typedef {Record<string, LockEntry>} Lock */
/** @typedef {{id: string, [key: string]: unknown}} Source */

/**
 * Why a lock cannot build the catalog: a source missing or not pinned to a
 * full SHA, or an entry for a source that no longer exists.
 * @param {Source[]} sources
 * @param {Lock} lock
 */
function lockProblems(sources, lock) {
  const problems = [];
  for (const {id} of sources) {
    const entry = lock[id];
    if (!entry) problems.push(`${id}: missing from skills.lock.json`);
    else if (!/^[0-9a-f]{40}$/.test(entry.commit)) problems.push(`${id}: commit "${entry.commit}" is not a 40-character SHA`);
  }
  const ids = new Set(sources.map((s) => s.id));
  for (const id of Object.keys(lock)) {
    if (!ids.has(id)) problems.push(`${id}: not a source in fetch-skills-catalog.mjs`);
  }
  return problems;
}

/**
 * The lock after an update: each source pinned to its branch head, with the
 * fetched star count, or the previous one when the API gave none.
 * @param {Source[]} sources
 * @param {Lock} prev
 * @param {Record<string, string>} heads
 * @param {Record<string, number | null>} stars
 * @returns {Lock}
 */
function nextLock(sources, prev, heads, stars) {
  return Object.fromEntries(
    sources.map(({id}) => [id, {commit: heads[id], stars: stars[id] ?? prev[id]?.stars ?? null}]),
  );
}

/**
 * Names one source's cached extract. It changes when the commit, the source's
 * config or the generator code changes, so a stale extract is never reused.
 * @param {Source} source
 * @param {string} commit
 * @param {string} codeHash
 */
function sourceCacheKey(source, commit, codeHash) {
  const hash = createHash('sha256').update(JSON.stringify(source)).update(commit).update(codeHash).digest('hex');
  return `${source.id}-${hash.slice(0, 16)}`;
}

module.exports = {lockProblems, nextLock, sourceCacheKey};
