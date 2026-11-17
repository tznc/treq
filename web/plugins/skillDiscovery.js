// @ts-check
// Pure helpers the catalog script (scripts/fetch-skills-catalog.mjs) uses to
// find a source's skills among every SKILL.md path in its repo, and to label
// their licenses.

/**
 * The root patterns of a source: `skillsRoots` when it lists several, else
 * its single `skillsRoot` ('' is the repo root).
 * @param {{skillsRoots?: string[], skillsRoot?: string}} source
 */
function sourceRoots(source) {
  return source.skillsRoots ?? [source.skillsRoot ?? ''];
}

/** @param {string} path */
const segmentsOf = (path) => path.split('/').filter(Boolean);

/**
 * Matches a folder against a root pattern. A `*` segment matches one folder
 * that is not hidden. Returns the folder's path under the root, with the
 * folders `*` matched kept in front (so `plugins/*\/skills` turns
 * `plugins/discord/skills/access` into `discord/access`), or null when the
 * folder is not strictly inside the root.
 * @param {string[]} dir
 * @param {string[]} pattern
 */
function matchRoot(dir, pattern) {
  if (dir.length <= pattern.length) return null;
  const kept = [];
  for (const [i, part] of pattern.entries()) {
    if (part === '*') {
      if (dir[i].startsWith('.')) return null;
      kept.push(dir[i]);
    } else if (part !== dir[i]) {
      return null;
    }
  }
  return [...kept, ...dir.slice(pattern.length)].join('/');
}

/**
 * Picks a source's skill folders from the paths of every SKILL.md in its
 * repo. Each SKILL.md belongs to the first root that holds it. Skill ids and
 * routes come from `relDir`, so when two roots hold a skill at the same
 * `relDir` (a mirrored copy, say), the earlier root wins and the later one
 * is skipped.
 * @param {string[]} skillFiles repo-relative paths
 * @param {string[]} roots root patterns, in priority order
 */
function discoverSkills(skillFiles, roots) {
  const files = skillFiles.map((file) => file.replace(/\\/g, '/')).sort();
  /** @type {Array<{file: string, dir: string, relDir: string}>} */
  const skills = [];
  /** @type {Array<{file: string, relDir: string}>} */
  const skipped = [];
  const claimed = new Set();
  const taken = new Set();
  for (const root of roots) {
    const pattern = segmentsOf(root);
    for (const file of files) {
      if (claimed.has(file)) continue;
      const dir = segmentsOf(file).slice(0, -1);
      const relDir = matchRoot(dir, pattern);
      if (relDir === null) continue;
      claimed.add(file);
      if (taken.has(relDir)) {
        skipped.push({file, relDir});
        continue;
      }
      taken.add(relDir);
      skills.push({file, dir: dir.join('/'), relDir});
    }
  }
  return {skills, skipped};
}

/**
 * The folders to look in for a skill's license file, nearest first: the
 * skill folder, then each parent up to the repo root (''). Plugin repos keep
 * a LICENSE per plugin that can differ from the repo's.
 * @param {string} dir repo-relative skill folder
 */
function licenseDirs(dir) {
  const parts = segmentsOf(dir);
  return parts.map((_, i) => parts.slice(0, parts.length - i).join('/')).concat('');
}

// Detects a short SPDX-ish label from license file text. Skill folders often
// ship a LICENSE.txt whose SKILL.md frontmatter only says "see LICENSE.txt" —
// this reads the actual text so the UI can show "Apache-2.0" / "MIT" /
// "Proprietary" instead of that unhelpful pointer.
/** @param {string} text */
function detectLicense(text) {
  const head = text.slice(0, 3000);
  if (/Apache License[\s\S]{0,60}Version 2\.0/i.test(head)) return 'Apache-2.0';
  if (
    /\bMIT License\b/i.test(head) ||
    /Permission is hereby granted, free of charge, to any person obtaining a copy/i.test(head)
  ) return 'MIT';
  if (/BSD 3-Clause/i.test(head) || (/Redistribution and use in source and binary forms/i.test(head) && /neither the name/i.test(head))) {
    return 'BSD-3-Clause';
  }
  if (/BSD 2-Clause/i.test(head)) return 'BSD-2-Clause';
  // Before the GPL: MPL-2.0 names the GPL as a "Secondary License".
  if (/Mozilla Public License/i.test(head)) return 'MPL-2.0';
  if (/GNU GENERAL PUBLIC LICENSE[\s\S]{0,60}Version 3/i.test(head)) return 'GPL-3.0';
  if (/GNU GENERAL PUBLIC LICENSE[\s\S]{0,60}Version 2/i.test(head)) return 'GPL-2.0';
  if (/\bISC License\b/i.test(head)) return 'ISC';
  if (/\bThe Unlicense\b/i.test(head)) return 'Unlicense';
  if (/all rights reserved/i.test(head) && /(commercial terms|consumer terms|additional restrictions|proprietary)/i.test(head)) {
    return 'Proprietary';
  }
  return 'Custom';
}

module.exports = {sourceRoots, discoverSkills, licenseDirs, detectLicense};
