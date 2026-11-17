import { APP_DEEP_LINK } from '../../lib/constants';

// Extensions confirmed present in prism-react-renderer's default language
// bundle. Anything else falls back to plain, unhighlighted text rather than
// risking a runtime error from an unregistered Prism grammar.
const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  md: 'markdown',
  mdx: 'markdown',
  py: 'python',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'jsx',
  ts: 'typescript',
  tsx: 'tsx',
  json: 'json',
  ipynb: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  xml: 'markup',
  xsd: 'markup',
  html: 'markup',
  htm: 'markup',
  css: 'css',
  swift: 'swift',
  sql: 'sql',
  go: 'go',
  rs: 'rust',
  graphql: 'graphql',
  gql: 'graphql',
  kt: 'kotlin',
  kts: 'kotlin',
  c: 'c',
  cpp: 'cpp',
  cc: 'cpp',
  h: 'cpp',
  hpp: 'cpp',
  m: 'objectivec',
};

export function getPrismLanguage(path: string): string | null {
  const ext = path.split('.').pop()?.toLowerCase();
  if (!ext) return null;
  return LANGUAGE_BY_EXTENSION[ext] ?? null;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// Parsed by `parseSkillInstallDeepLink` in the app (src/lib/skillDeepLink.ts).
export function skillInstallDeepLink(skillId: string): string {
  return `${APP_DEEP_LINK}skill/install?id=${encodeURIComponent(skillId)}`;
}
