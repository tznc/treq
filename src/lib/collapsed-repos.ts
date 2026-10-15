const collapsedReposKey = (mainRepoPath?: string) =>
  `treq.sidebar.collapsedRepos:${mainRepoPath ?? ""}`;

/** Repositories whose sidebar workspace group is folded, per main repository. */
export const readCollapsedRepos = (mainRepoPath?: string): Set<string> => {
  try {
    const raw = localStorage.getItem(collapsedReposKey(mainRepoPath));
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
};

export const writeCollapsedRepos = (
  mainRepoPath: string | undefined,
  paths: Set<string>,
) => {
  try {
    localStorage.setItem(
      collapsedReposKey(mainRepoPath),
      JSON.stringify([...paths]),
    );
  } catch {
    // Collapsed state is a convenience; ignore storage failures.
  }
};
