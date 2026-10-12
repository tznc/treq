import useSWR, { useSWRConfig } from "swr";
import { getRepoSetting, setRepoSetting } from "../lib/api";
import {
  type AgentEnvironment,
  type DevcontainerStatus,
  ENVIRONMENT_SETTING,
  devcontainerStatus,
  devcontainerUp,
} from "../lib/api-devcontainer";
import { usePreviewFeature } from "../stores/featurePreviewStore";

export interface RepoEnvironment {
  /** Where new terminals and agent sessions start in this repository. */
  environment: AgentEnvironment;
  status: DevcontainerStatus | undefined;
  /** `devcontainer up` is running for this repository. */
  starting: boolean;
  setEnvironment: (next: AgentEnvironment) => Promise<void>;
  startDevcontainer: () => Promise<void>;
}

/**
 * The repository's environment setting and its devcontainer's state, shared
 * by every picker for the same repository. `null` while the preview is off or
 * no repository is open.
 */
export function useRepoEnvironment(
  repoPath: string | null | undefined,
  onError: (message: string) => void,
): RepoEnvironment | null {
  const enabled = usePreviewFeature("devcontainers") && !!repoPath;
  const { mutate } = useSWRConfig();
  const settingKey = enabled
    ? ["repo-setting", repoPath, ENVIRONMENT_SETTING]
    : null;
  const statusKey = enabled ? ["devcontainer-status", repoPath] : null;
  const startingKey = enabled ? ["devcontainer-starting", repoPath] : null;

  const { data: setting } = useSWR(settingKey, () =>
    getRepoSetting(repoPath!, ENVIRONMENT_SETTING),
  );
  const { data: status } = useSWR(statusKey, () =>
    devcontainerStatus(repoPath!),
  );
  const { data: starting = false } = useSWR<boolean>(startingKey, null);

  if (!enabled) return null;

  const startDevcontainer = async () => {
    await mutate(startingKey, true, { revalidate: false });
    try {
      const next = await devcontainerUp(repoPath);
      await mutate(statusKey, next, { revalidate: false });
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
      await mutate(statusKey);
    } finally {
      await mutate(startingKey, false, { revalidate: false });
    }
  };

  const setEnvironment = async (next: AgentEnvironment) => {
    await setRepoSetting(repoPath, ENVIRONMENT_SETTING, next);
    await mutate(settingKey, next, { revalidate: false });
    if (next === "devcontainer" && status && !status.running && !starting) {
      await startDevcontainer();
    }
  };

  return {
    environment: setting === "devcontainer" ? "devcontainer" : "local",
    status,
    starting,
    setEnvironment,
    startDevcontainer,
  };
}
