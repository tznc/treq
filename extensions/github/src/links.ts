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

/** Claude Code on the web, with `repo` preselected and `prompt` prefilled. */
export const claudePromptUrl = (repo: string, prompt: string): string => `${claudeUrl(repo)}&prompt=${enc(prompt)}`;

/** A new task in the Codex app with `prompt` in its composer. */
export const codexAppUrl = (prompt: string): string => `codex://new?prompt=${enc(prompt)}`;

/** Starts an agent with `prompt` in Treq, on `branch` of GitHub repository `repo`. */
export const treqKickoffUrl = (repo: string, branch: string, prompt: string): string =>
  `treq://agent/kickoff?repo=${enc(repo)}&branch=${enc(branch)}&prompt=${enc(prompt)}`;
