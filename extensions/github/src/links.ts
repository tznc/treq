const enc = encodeURIComponent;

/** Claude Code on the web, with `repo` preselected. */
export const claudeUrl = (repo: string): string => `https://claude.ai/code?repositories=${enc(repo)}`;

/** Codex on the web: it takes no repository or prompt in its URL. */
export const CODEX_WEB_URL = "https://chatgpt.com/codex";

/** Focuses the Treq window open on GitHub repository `repo`. */
export const treqRepoUrl = (repo: string): string => `treq://repo/open?repo=${enc(repo)}`;

/** Shows pull request `number` of `repo` in Treq. */
export const treqPrUrl = (repo: string, number: number): string =>
  `treq://pr/open?repo=${enc(repo)}&number=${number}`;
