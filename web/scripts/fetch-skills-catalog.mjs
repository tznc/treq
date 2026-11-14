// Regenerates web/src/data/skills.json by cloning curated Agent Skill repos
// (shallow, to a scratch dir), parsing each SKILL.md's frontmatter, and
// indexing each skill folder's files (path + size only). It also writes
// web/src/data/skill-pages.json: the SKILL.md body of every skill whose
// license permits redistribution, which the skill page renders into static
// HTML with credit to the author. The contents of other files are fetched live
// from GitHub when a user browses a skill. Neither is fed into the sitewide
// search index.
//
// Run manually with `npm run build:skills-catalog`, or automatically by the
// "Update Skills Catalog" GitHub Action (see .github/workflows).
import { execFileSync } from 'child_process';
import { mkdtempSync, readdirSync, statSync, readFileSync, writeFileSync, rmSync, existsSync } from 'fs';
import { join, relative, dirname } from 'path';
import { tmpdir } from 'os';
import { fileURLToPath } from 'url';
import { createHash } from 'crypto';
import matter from 'gray-matter';
import skillPageContent from '../plugins/skillPageContent.js';

const { canRedistribute } = skillPageContent;

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const webRoot = join(__dirname, '..');
const outPath = join(webRoot, 'src', 'data', 'skills.json');
const pagesOutPath = join(webRoot, 'src', 'data', 'skill-pages.json');

// Reads star counts from the previously-generated catalog so a regen in an
// environment that can't reach api.github.com (e.g. behind a restrictive
// egress proxy) preserves them instead of wiping them to null.
function readPreviousStars() {
  if (!existsSync(outPath)) return {};
  try {
    const prev = JSON.parse(readFileSync(outPath, 'utf8'));
    return Object.fromEntries((prev.sources ?? []).map((s) => [s.id, s.stars ?? null]));
  } catch {
    return {};
  }
}

// Live GitHub star count for a repo. Returns null on any failure (rate limit,
// network, blocked host) so the build never fails over stars. In CI, set
// GITHUB_TOKEN to raise the unauthenticated rate limit.
async function fetchStars(source) {
  const headers = {
    Accept: 'application/vnd.github+json',
    'User-Agent': 'treq-skills-catalog',
  };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  try {
    const res = await fetch(`https://api.github.com/repos/${source.org}/${source.repo}`, { headers });
    if (!res.ok) return null;
    const data = await res.json();
    return typeof data.stargazers_count === 'number' ? data.stargazers_count : null;
  } catch {
    return null;
  }
}

// Curated sources. `id` is the stable provider key used in routes and the
// provider filter; `org`/`repo` locate the GitHub repo; `skillsRoot` is the
// path within the repo that holds one `SKILL.md` per skill folder (per the
// Agent Skills spec https://github.com/anthropics/skills) — use '' when skills
// live at the repo root.
const SOURCES = [
  {
    id: 'anthropic',
    name: 'Anthropic',
    org: 'anthropics',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'openai',
    name: 'OpenAI',
    org: 'openai',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'mattpocock',
    name: 'Matt Pocock',
    org: 'mattpocock',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    // Skills the author has explicitly marked as superseded/unfinished.
    excludeCategories: ['deprecated'],
  },
  {
    id: 'vercel',
    name: 'Vercel',
    org: 'vercel-labs',
    repo: 'agent-browser',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'microsoft',
    name: 'Microsoft',
    org: 'microsoft',
    repo: 'azure-skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'superpowers',
    name: 'Superpowers',
    org: 'obra',
    repo: 'superpowers',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'caveman',
    name: 'Caveman',
    org: 'JuliusBrussee',
    repo: 'caveman',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'prisma',
    name: 'Prisma',
    org: 'prisma',
    repo: 'skills',
    branch: 'main',
    // Prisma keeps each skill folder at the repo root, not under skills/.
    skillsRoot: '',
    excludeCategories: [],
  },
];

// Mirrors the isBinaryFile() extension list in src/components/FileBrowser.tsx
// so the web skills browser treats files the same way the desktop app does.
const BINARY_EXTENSIONS = [
  '.png', '.jpg', '.jpeg', '.gif', '.bmp', '.ico', '.svg', '.pdf',
  '.zip', '.tar', '.gz', '.rar', '.7z', '.exe', '.dll', '.so', '.dylib',
  '.mp3', '.mp4', '.avi', '.mov', '.wmv', '.woff', '.woff2', '.ttf', '.eot',
];

function isBinaryFile(path) {
  const lower = path.toLowerCase();
  return BINARY_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

// Detects a short SPDX-ish label from license file text. Skill folders often
// ship a LICENSE.txt whose SKILL.md frontmatter only says "see LICENSE.txt" —
// this reads the actual text so the UI can show "Apache-2.0" / "MIT" /
// "Proprietary" instead of that unhelpful pointer.
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
  if (/GNU GENERAL PUBLIC LICENSE[\s\S]{0,60}Version 3/i.test(head)) return 'GPL-3.0';
  if (/GNU GENERAL PUBLIC LICENSE[\s\S]{0,60}Version 2/i.test(head)) return 'GPL-2.0';
  if (/Mozilla Public License/i.test(head)) return 'MPL-2.0';
  if (/\bISC License\b/i.test(head)) return 'ISC';
  if (/\bThe Unlicense\b/i.test(head)) return 'Unlicense';
  if (/all rights reserved/i.test(head) && /(commercial terms|consumer terms|additional restrictions|proprietary)/i.test(head)) {
    return 'Proprietary';
  }
  return 'Custom';
}

function findLicenseFile(dir) {
  const match = readdirSync(dir).find((entry) => /^license(\.(txt|md))?$/i.test(entry));
  return match ? join(dir, match) : null;
}

// The detected license label and the license file's path, or null.
function detectDirLicense(dir) {
  const file = findLicenseFile(dir);
  if (!file) return null;
  return { license: detectLicense(readFileSync(file, 'utf8')), file };
}

function humanize(segment) {
  return segment
    .replace(/^\./, '')
    .replace(/[-_]/g, ' ')
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

function slugifySegment(segment) {
  return segment.replace(/^\./, '');
}

function findSkillFiles(dir, results = []) {
  for (const entry of readdirSync(dir)) {
    if (entry === '.git') continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      findSkillFiles(full, results);
    } else if (entry === 'SKILL.md') {
      results.push(full);
    }
  }
  return results;
}

function dirHasSkillFile(dir) {
  return readdirSync(dir).includes('SKILL.md');
}

function walkFiles(dir, base, results = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      // A subdirectory with its own SKILL.md is a separate (nested) skill —
      // its files belong to that skill, not this one, so don't descend.
      if (dirHasSkillFile(full)) continue;
      walkFiles(full, base, results);
    } else {
      results.push(relative(base, full).replace(/\\/g, '/'));
    }
  }
  return results;
}

function cloneSource(source, scratchDir) {
  const dest = join(scratchDir, source.id);
  execFileSync(
    'git',
    ['clone', '--depth', '1', '--branch', source.branch, `https://github.com/${source.org}/${source.repo}.git`, dest],
    { stdio: 'inherit' },
  );
  return dest;
}

function skillChecksum(skillDir, relPaths) {
  const hash = createHash('sha256');
  for (const relPath of [...relPaths].sort()) {
    const content = readFileSync(join(skillDir, relPath));
    hash.update(relPath);
    hash.update(Buffer.from([0]));
    const len = Buffer.alloc(8);
    len.writeBigUInt64BE(BigInt(content.length));
    hash.update(len);
    hash.update(content);
  }
  return hash.digest('hex');
}

function buildFileManifest(source, skillDir, repoRelPath) {
  return walkFiles(skillDir, skillDir)
    .sort()
    .map((relPath) => {
      const repoPath = `${repoRelPath}/${relPath}`;
      return {
        path: relPath,
        size: statSync(join(skillDir, relPath)).size,
        binary: isBinaryFile(relPath),
        githubUrl: `https://github.com/${source.org}/${source.repo}/blob/${source.branch}/${repoPath}`,
        rawUrl: `https://raw.githubusercontent.com/${source.org}/${source.repo}/${source.branch}/${repoPath}`,
      };
    });
}

function extractSkills(source, cloneDir, rootLicense, pages) {
  const skillsRoot = join(cloneDir, source.skillsRoot);
  const skills = [];
  const blobUrl = (file) =>
    `https://github.com/${source.org}/${source.repo}/blob/${source.branch}/${relative(cloneDir, file).replace(/\\/g, '/')}`;

  for (const skillFile of findSkillFiles(skillsRoot)) {
    const skillDir = dirname(skillFile);
    const relDir = relative(skillsRoot, skillDir).replace(/\\/g, '/');
    const segments = relDir.split('/');
    const category = segments.length > 1 ? segments[0] : null;

    if (category && source.excludeCategories.includes(category)) continue;

    const raw = readFileSync(skillFile, 'utf8');
    let parsed;
    try {
      parsed = matter(raw);
    } catch (e) {
      // One malformed SKILL.md upstream shouldn't fail the whole catalog.
      console.warn(`  skipping ${relDir}: invalid frontmatter (${e.reason ?? e.message})`);
      continue;
    }
    const { data: frontmatter, content: body } = parsed;
    if (!frontmatter.name || !frontmatter.description) continue;

    const repoRelPath = relative(cloneDir, skillDir).replace(/\\/g, '/');
    const routeSlug = segments.map(slugifySegment).join('/');
    const licenseInfo = detectDirLicense(skillDir) ?? rootLicense;
    const license = licenseInfo?.license ?? null;

    // Proprietary/source-available skills (e.g. Anthropic's docx/pdf/pptx/xlsx)
    // don't permit us to reproduce their contents, so we still list them but
    // withhold the description and file manifest — the UI shows a notice and a
    // link to GitHub instead. This also keeps their text out of the search
    // index (which only reads description/name).
    const proprietary = license === 'Proprietary';
    const files = proprietary ? [] : buildFileManifest(source, skillDir, repoRelPath);
    const checksum = proprietary ? null : skillChecksum(skillDir, files.map((file) => file.path));

    const id = `${source.id}/${relDir}`;
    pages[id] = {
      body: !proprietary && canRedistribute(license) ? body.trim() : null,
      licenseUrl: licenseInfo ? blobUrl(licenseInfo.file) : null,
      manualOnly: frontmatter['disable-model-invocation'] === true,
    };

    skills.push({
      id,
      name: frontmatter.name,
      description: proprietary ? null : String(frontmatter.description).trim(),
      source: source.id,
      category: category ? humanize(category) : null,
      license,
      proprietary,
      path: repoRelPath,
      url: `https://github.com/${source.org}/${source.repo}/tree/${source.branch}/${repoRelPath}`,
      route: `/skills/${source.id}/${routeSlug}`,
      checksum,
      files,
    });
  }

  return skills;
}

async function main() {
  const scratchDir = mkdtempSync(join(tmpdir(), 'skills-catalog-'));
  const previousStars = readPreviousStars();
  const allSkills = [];
  const sourceMeta = [];
  const pages = {};

  try {
    for (const source of SOURCES) {
      console.log(`Fetching ${source.org}/${source.repo}...`);
      const cloneDir = cloneSource(source, scratchDir);
      const rootLicense = detectDirLicense(cloneDir);
      const skills = extractSkills(source, cloneDir, rootLicense, pages);
      allSkills.push(...skills);
      const stars = (await fetchStars(source)) ?? previousStars[source.id] ?? null;
      sourceMeta.push({
        id: source.id,
        name: source.name,
        url: `https://github.com/${source.org}/${source.repo}`,
        skillCount: skills.length,
        stars,
      });
      console.log(`  found ${skills.length} skills (${stars ?? '?'} stars)`);
    }
  } finally {
    rmSync(scratchDir, { recursive: true, force: true });
  }

  allSkills.sort((a, b) =>
    a.source.localeCompare(b.source) ||
    (a.category ?? '').localeCompare(b.category ?? '') ||
    a.name.localeCompare(b.name),
  );

  const catalog = {
    generatedAt: new Date().toISOString(),
    sources: sourceMeta,
    skills: allSkills,
  };

  writeFileSync(outPath, JSON.stringify(catalog, null, 2) + '\n');
  const sortedPages = Object.fromEntries(allSkills.map((skill) => [skill.id, pages[skill.id]]));
  writeFileSync(pagesOutPath, JSON.stringify(sortedPages, null, 2) + '\n');
  console.log(`\nWrote ${allSkills.length} skills from ${sourceMeta.length} sources to ${relative(webRoot, outPath)}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
