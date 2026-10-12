import { invoke } from "@tauri-apps/api/core";

export interface DevcontainerStatus {
  /** The repository has a devcontainer config. */
  configured: boolean;
  /** Path of the config, relative to the repository. */
  configPath: string | null;
  /** The Dev Containers CLI is installed on this machine. */
  cliAvailable: boolean;
  /** Treq's container for this repository is running. */
  running: boolean;
}

export type AgentEnvironment = "local" | "devcontainer";

/** Repo setting that picks where new terminals and agent sessions run. */
export const ENVIRONMENT_SETTING = "environment";

export const devcontainerStatus = (
  repoPath: string,
): Promise<DevcontainerStatus> => invoke("devcontainer_status", { repoPath });

/** Builds (if needed) and starts the repository's devcontainer. */
export const devcontainerUp = (repoPath: string): Promise<DevcontainerStatus> =>
  invoke("devcontainer_up", { repoPath });
