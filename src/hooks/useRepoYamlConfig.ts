import { useState } from "react";
import useSWR from "swr";
import { loadRepoYamlConfig } from "../lib/api";
import type { RepoYamlConfig } from "../lib/api-extra";
import { invalidateQueries } from "../lib/swr-cache";

const repoYamlConfigKey = (repoPath: string | undefined) =>
  repoPath ? (["repo-yaml-config", repoPath] as const) : null;

/**
 * Parses `.treq/config.yaml` via SWR, keyed by repo path so every caller
 * sharing that key sees the same data and a `reload()` from any one of them
 * revalidates it for all. Loading it also writes matching fields into the
 * repo settings store on the backend, so this also revalidates the
 * `repo-settings` SWR key that `RepositorySettingsContent` reads from.
 *
 * `loading` stays true until one fetch has finished since this hook mounted
 * (or since `repoPath` changed). With a result cached by an earlier mount,
 * SWR returns it at once with `isLoading` false and only starts the refetch
 * on the next animation frame. Until that refetch ends, the cached config
 * and the repo settings it synced can be stale: fields from a file written
 * since the last mount would render enabled, with their old values.
 */
export function useRepoYamlConfig(repoPath: string | undefined) {
  const { data, error, isLoading, isValidating, mutate } = useSWR(
    repoYamlConfigKey(repoPath),
    async () => {
      const config = await loadRepoYamlConfig(repoPath!);
      await invalidateQueries(["repo-settings", repoPath]);
      return config;
    },
    // Re-reading a small local YAML file is cheap; always refetch on mount
    // (e.g. reopening Settings) instead of reusing a recent cached result.
    { dedupingInterval: 0 },
  );

  // Tracks one full validation cycle since mount; see the doc comment above.
  const [mountSync, setMountSync] = useState({
    repoPath,
    started: false,
    done: false,
  });
  let sync = mountSync;
  if (sync.repoPath !== repoPath) {
    sync = { repoPath, started: false, done: false };
  }
  if (!sync.done) {
    if (isValidating && !sync.started) sync = { ...sync, started: true };
    else if (!isValidating && sync.started) sync = { ...sync, done: true };
  }
  if (sync !== mountSync) setMountSync(sync);

  return {
    config: data as RepoYamlConfig | undefined,
    loading: repoPath != null && (isLoading || !sync.done),
    error: error
      ? `Failed to load .treq/config.yaml: ${error instanceof Error ? error.message : String(error)}`
      : null,
    reload: () => mutate(),
  };
}
