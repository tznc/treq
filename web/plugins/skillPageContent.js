// @ts-check
// Pure helpers for the content of each /skills/<author>/<skill> page. The
// catalog script uses canRedistribute() to decide which SKILL.md bodies to
// store, and skillsPlugin.js uses the rest to build each page's props at
// build time, so search engines get the text in the static HTML.

// Licenses that let anyone copy the text as long as the copyright and license
// notice travel with it. Each page credits the author and links the license
// file. "Custom" and "Proprietary" bodies are never stored or shown.
const REDISTRIBUTABLE_LICENSES = new Set(['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Unlicense']);

/** @param {string | null | undefined} license */
function canRedistribute(license) {
  return typeof license === 'string' && REDISTRIBUTABLE_LICENSES.has(license);
}

/**
 * Shortens text for a meta description. A sentence end is preferred when it
 * keeps at least 60 characters, otherwise the cut falls on a word boundary
 * and gets an ellipsis.
 * @param {string | null | undefined} text
 * @param {number} [max]
 */
function truncateDescription(text, max = 160) {
  const clean = (text ?? '').replace(/\s+/g, ' ').trim();
  if (clean.length <= max) return clean;
  const head = clean.slice(0, max);
  const sentenceEnd = Math.max(head.lastIndexOf('. '), head.lastIndexOf('? '), head.lastIndexOf('! '));
  if (sentenceEnd >= 60) return head.slice(0, sentenceEnd + 1);
  const room = clean.slice(0, max - 1);
  const lastSpace = room.lastIndexOf(' ');
  const cut = lastSpace > 0 ? room.slice(0, lastSpace) : room;
  return `${cut.replace(/[\s,;:.\-–—(]+$/, '')}…`;
}

/**
 * Up to `limit` other skills: same author and category first, then the rest
 * of the author's skills, then other authors' skills in the same category.
 * @param {{id: string, source: string, category: string | null}} skill
 * @param {Array<{id: string, name: string, description: string | null, source: string, category: string | null, route: string}>} skills
 * @param {Array<{id: string, name: string}>} sources
 * @param {number} [limit]
 */
function relatedSkills(skill, skills, sources, limit = 6) {
  /** @param {typeof skills[number]} other */
  const rank = (other) => {
    const sameAuthor = other.source === skill.source;
    const sameCategory = other.category === skill.category;
    if (sameAuthor && sameCategory) return 0;
    if (sameAuthor) return 1;
    if (sameCategory && skill.category !== null) return 2;
    return 3;
  };
  return skills
    .filter((other) => other.id !== skill.id && rank(other) < 3)
    .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
    .slice(0, limit)
    .map((other) => ({
      name: other.name,
      route: other.route,
      author: sources.find((s) => s.id === other.source)?.name ?? other.source,
      description: truncateDescription(other.description, 120),
    }));
}

/**
 * Shell commands that copy one skill folder into the project-level skills
 * directory Claude Code reads (.claude/skills) and the one Codex reads
 * (.agents/skills).
 * @param {{path: string}} skill
 * @param {{url: string}} source
 */
function installCommands(skill, source) {
  const [org, repo] = source.url.replace('https://github.com/', '').split('/');
  const clone = `/tmp/${org}-${repo}`;
  const folder = skill.path.split('/').pop() ?? skill.path;
  /** @param {string} dir */
  const commands = (dir) =>
    [`git clone --depth 1 ${source.url}.git ${clone}`, `mkdir -p ${dir}`, `cp -r ${clone}/${skill.path} ${dir}/`].join('\n');
  return {folder, claudeCode: commands('.claude/skills'), codex: commands('.agents/skills')};
}

const TRIGGER = /\b(?:use (?:this skill |this |it )?(?:only )?(?:when|whenever|for|if|before|after)\b|when:|triggers? (?:on|when)\b)/i;

/**
 * The sentence of a skill description that says when to use it, if any.
 * @param {string | null | undefined} description
 */
function triggerClause(description) {
  const text = (description ?? '').replace(/\s+/g, ' ').trim();
  const match = TRIGGER.exec(text);
  if (!match) return null;
  const rest = text.slice(match.index);
  const end = rest.search(/[.!?](\s|$)/);
  return truncateDescription(end === -1 ? rest : rest.slice(0, end + 1), 240);
}

module.exports = {canRedistribute, truncateDescription, relatedSkills, installCommands, triggerClause};
