const pathParts = (path: string): string[] =>
  path.split(/[\\/]/).filter(Boolean);

/**
 * Sidebar label for each repository path: its directory name, or
 * `parent/name` when two repositories share a directory name.
 */
export const repoDisplayLabels = (paths: string[]): Map<string, string> => {
  const nameCounts = new Map<string, number>();
  for (const path of paths) {
    const name = pathParts(path).at(-1) ?? path;
    nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1);
  }
  return new Map(
    paths.map((path) => {
      const parts = pathParts(path);
      const name = parts.at(-1) ?? path;
      const label =
        (nameCounts.get(name) ?? 0) > 1 && parts.length > 1
          ? parts.slice(-2).join("/")
          : name;
      return [path, label];
    }),
  );
};
