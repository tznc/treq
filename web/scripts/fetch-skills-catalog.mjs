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
import { mkdtempSync, readdirSync, statSync, lstatSync, readFileSync, writeFileSync, rmSync, existsSync } from 'fs';
import { join, relative, dirname } from 'path';
import { tmpdir } from 'os';
import { fileURLToPath } from 'url';
import { createHash } from 'crypto';
import matter from 'gray-matter';
import skillPageContent from '../plugins/skillPageContent.js';
import skillDiscovery from '../plugins/skillDiscovery.js';

const { canRedistribute } = skillPageContent;
const { sourceRoots, discoverSkills, licenseDirs, detectLicense } = skillDiscovery;

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const webRoot = join(__dirname, '..');
const outPath = join(webRoot, 'src', 'data', 'skills.json');
const pagesOutPath = join(webRoot, 'src', 'data', 'skill-pages.json');
const indexOutPath = join(webRoot, 'src', 'data', 'skills-index.json');

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
// live at the repo root. A repo that keeps skills in several folders lists
// them in `skillsRoots` instead, in priority order; a `*` segment matches one
// folder (e.g. 'plugins/*/skills') and that folder becomes the category. When
// two roots hold a skill at the same path, the first root wins (see
// plugins/skillDiscovery.js). A SKILL.md at a root itself is never a skill.
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
  {
    id: 'mvanhorn',
    name: 'Matt Van Horn',
    org: 'mvanhorn',
    repo: 'last30days-skill',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'palkan',
    name: 'Vladimir Dementyev',
    org: 'palkan',
    repo: 'layered-rails-skills',
    branch: 'master',
    skillsRoot: 'layered-rails/skills',
    excludeCategories: [],
  },
  {
    id: 'addyosmani',
    name: 'Addy Osmani',
    org: 'addyosmani',
    repo: 'agent-skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'impeccable',
    name: 'Impeccable',
    org: 'pbakaus',
    repo: 'impeccable',
    branch: 'main',
    // Impeccable ships the same skill to many agent folders. plugin/skills holds one copy.
    skillsRoot: 'plugin/skills',
    excludeCategories: [],
  },
  {
    id: 'taste-skill',
    name: 'Taste Skill',
    org: 'Leonxlnx',
    repo: 'taste-skill',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'emilkowalski',
    name: 'Emil Kowalski',
    org: 'emilkowalski',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'cathrynlavery',
    name: 'Cathryn Lavery',
    org: 'cathrynlavery',
    repo: 'diagram-design',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'ponytail',
    name: 'Ponytail',
    org: 'DietrichGebert',
    repo: 'ponytail',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'payload',
    name: 'Payload',
    org: 'payloadcms',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'huggingface',
    name: 'Hugging Face',
    org: 'huggingface',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'cloudflare',
    name: 'Cloudflare',
    org: 'cloudflare',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'supabase',
    name: 'Supabase',
    org: 'supabase',
    repo: 'agent-skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'expo',
    name: 'Expo',
    org: 'expo',
    repo: 'skills',
    branch: 'main',
    // Expo's stable skills. plugins/expo-experiments is left out.
    skillsRoot: 'plugins/expo/skills',
    excludeCategories: [],
  },
  {
    id: 'gemini',
    name: 'Google Gemini',
    org: 'google-gemini',
    repo: 'gemini-skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'sentry',
    name: 'Sentry',
    org: 'getsentry',
    repo: 'skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'google',
    name: 'Google',
    org: 'google',
    repo: 'skills',
    branch: 'main',
    // plugins/cloud holds plugin variants of skills that skills/ already lists.
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'claude-plugins',
    name: 'Claude Plugins',
    org: 'anthropics',
    repo: 'claude-plugins-official',
    branch: 'main',
    skillsRoots: ['plugins/*/skills', 'external_plugins/*/skills'],
    excludeCategories: [],
  },
  {
    id: 'hashicorp',
    name: 'HashiCorp',
    org: 'hashicorp',
    repo: 'agent-skills',
    branch: 'main',
    skillsRoot: 'plugins/*/skills',
    excludeCategories: [],
  },
  {
    id: 'stitch',
    name: 'Google Stitch',
    org: 'google-labs-code',
    repo: 'stitch-skills',
    branch: 'main',
    skillsRoot: 'plugins/*/skills',
    excludeCategories: [],
  },
  {
    id: 'neon',
    name: 'Neon',
    org: 'neondatabase',
    repo: 'agent-skills',
    branch: 'main',
    // plugins/neon-postgres/skills is a copy of skills/.
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'firebase',
    name: 'Firebase',
    org: 'firebase',
    repo: 'agent-skills',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'obsidian',
    name: 'Obsidian',
    org: 'obsidianmd',
    repo: 'obsidian-skills',
    branch: 'main',
    skillsRoot: 'plugins/obsidian/skills',
    excludeCategories: [],
  },
  {
    id: 'cloudflare-security',
    name: 'Cloudflare Security',
    org: 'cloudflare',
    repo: 'security-audit-skill',
    branch: 'main',
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'remotion',
    name: 'Remotion',
    org: 'remotion-dev',
    repo: 'skills',
    branch: 'main',
    // No license file, so the catalog lists these without their SKILL.md text.
    skillsRoot: 'skills',
    excludeCategories: [],
  },
  {
    id: 'gstack',
    name: 'gstack',
    org: 'garrytan',
    repo: 'gstack',
    branch: 'main',
    // Skills are top-level folders. The rest are test fixtures, ports for
    // other hosts and browser scripts; the root SKILL.md is skipped.
    skillsRoot: '',
    excludeCategories: ['test', 'qm', 'capy', 'browser-skills', 'openclaw'],
  },
  {
    id: 'planning-with-files',
    name: 'Planning with Files',
    org: 'OthmanAdi',
    repo: 'planning-with-files',
    branch: 'master',
    // skills/i18n holds translations of the one skill.
    skillsRoot: 'skills',
    excludeCategories: ['i18n'],
  },
  {
    id: 'hallmark',
    name: 'Hallmark',
    org: 'Nutlope',
    repo: 'hallmark',
    branch: 'main',
    skillsRoot: 'skills',
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

// Symlinked folders are not followed: repos link one skill folder under a
// second name (gstack's connect-chrome), which would list it twice.
function findSkillFiles(dir, results = []) {
  for (const entry of readdirSync(dir)) {
    if (entry === '.git') continue;
    const full = join(dir, entry);
    if (lstatSync(full).isSymbolicLink()) continue;
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

// The nearest license file to a skill folder: its own, else a parent's.
function findLicense(cloneDir, repoRelDir, cache) {
  for (const dir of licenseDirs(repoRelDir)) {
    if (!cache.has(dir)) cache.set(dir, detectDirLicense(join(cloneDir, dir)));
    if (cache.get(dir)) return cache.get(dir);
  }
  return null;
}

function extractSkills(source, cloneDir, pages) {
  const skills = [];
  const licenseCache = new Map();
  const blobUrl = (file) =>
    `https://github.com/${source.org}/${source.repo}/blob/${source.branch}/${relative(cloneDir, file).replace(/\\/g, '/')}`;

  const skillFiles = findSkillFiles(cloneDir).map((file) => relative(cloneDir, file));
  const found = discoverSkills(skillFiles, sourceRoots(source));
  for (const { file, relDir } of found.skipped) console.warn(`  skipping ${file}: ${relDir} is already listed`);

  for (const { file, relDir } of found.skills) {
    const skillFile = join(cloneDir, file);
    const skillDir = dirname(skillFile);
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
    const licenseInfo = findLicense(cloneDir, repoRelPath, licenseCache);
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
      const skills = extractSkills(source, cloneDir, pages);
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
  // The /skills/ page bundles its data into client JS, so it gets a copy
  // without the file manifests, which only the skill pages and the app use.
  const index = { sources: sourceMeta, skills: allSkills.map(({ files, checksum, ...rest }) => rest) };
  writeFileSync(indexOutPath, JSON.stringify(index, null, 2) + '\n');
  const sortedPages = Object.fromEntries(allSkills.map((skill) => [skill.id, pages[skill.id]]));
  writeFileSync(pagesOutPath, JSON.stringify(sortedPages, null, 2) + '\n');
  console.log(`\nWrote ${allSkills.length} skills from ${sourceMeta.length} sources to ${relative(webRoot, outPath)}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
