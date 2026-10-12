import { initRepo, setSetting, setWindowRepoPath } from "./api";
import { invalidateQueries } from "./swr-cache";

interface OpenRepositoryToast {
  title: string;
  description: string;
  type: "success" | "error";
}

export interface OpenRepositoryCallbacks {
  addToast: (toast: OpenRepositoryToast) => void;
  onOpened: (path: string) => void;
}

/**
 * Initializes and switches to the repository at `path`, shared by both the
 * button-driven onboarding flow and the native "Open..." menu action (whose
 * path is resolved by the Rust side before this ever runs).
 */
export async function openRepositoryAtPath(
  path: string,
  { addToast, onOpened }: OpenRepositoryCallbacks,
): Promise<void> {
  try {
    await initRepo(path);
  } catch (error) {
    addToast({
      title: "Failed to open repository",
      description: error instanceof Error ? error.message : String(error),
      type: "error",
    });
    return;
  }

  await setSetting("last_opened_repo_path", path);
  await setWindowRepoPath(path);
  onOpened(path);

  void invalidateQueries(["workspaces"]);
  void invalidateQueries(["workspace-statuses"]);
  void invalidateQueries(["sessions"]);
  void invalidateQueries(["repo-status"]);
  void invalidateQueries(["repo-branch"]);

  addToast({
    title: "Repository Opened",
    description: `Now viewing ${path.split("/").pop() || path}`,
    type: "success",
  });
}
