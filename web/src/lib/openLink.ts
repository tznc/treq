export type PrRef = {owner: string; repo: string; number: number};

// GitHub owner and repository names: letters, digits, `-`, `_` and `.`,
// but never `.` or `..` alone.
const NAME = /^(?!\.{1,2}$)[A-Za-z0-9._-]+$/;

/** The PR a `/open/pr/{owner}/{repo}/{number}` path names, or null. */
export function parsePrPath(pathname: string): PrRef | null {
  const parts = pathname.replace(/\/$/, '').split('/');
  if (parts.length !== 6 || parts[0] !== '' || parts[1] !== 'open' || parts[2] !== 'pr') return null;
  const [, , , owner, repo, num] = parts;
  if (!NAME.test(owner) || !NAME.test(repo) || !/^[1-9]\d*$/.test(num)) return null;
  return {owner, repo, number: Number(num)};
}

/** The `treq://` link the desktop app opens the PR from. */
export function prDeepLink({owner, repo, number}: PrRef): string {
  return `treq://pr/open?repo=${encodeURIComponent(`${owner}/${repo}`)}&number=${number}`;
}

export function prGithubUrl({owner, repo, number}: PrRef): string {
  return `https://github.com/${owner}/${repo}/pull/${number}`;
}
