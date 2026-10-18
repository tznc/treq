import {
  createWorkspace,
  getRepoSetting,
  getWorkspaces,
  listRepoBranches,
} from "./api";
import { applyBranchNamePattern } from "./utils";
import { buildCreateMetadata } from "./workspaceMetadata";

/** Repo setting for the home-view "Run in a new workspace" toggle. */
export const NEW_WORKSPACE_FOR_TASKS_SETTING = "home_task_new_workspace";

/** On unless the repo turned it off. */
export const isNewWorkspaceForTasksOn = (value: string | null | undefined) =>
  value !== "false";

const MAX_SLUG_LENGTH = 48;

/**
 * Lowercase ASCII words joined by `-`, cut on a word boundary: the same shape
 * as the branch names Treq gives issue workspaces.
 */
function taskBranchSlug(prompt: string): string {
  const slug = (prompt.toLowerCase().match(/[a-z0-9]+/g) ?? []).join("-");
  if (slug.length <= MAX_SLUG_LENGTH) return slug;
  const cut = slug.slice(0, MAX_SLUG_LENGTH);
  const lastDash = cut.lastIndexOf("-");
  return lastDash > 0 ? cut.slice(0, lastDash) : cut;
}

/** A branch name for the task that no workspace or local branch uses yet. */
export function taskBranchName(
  prompt: string,
  taken: ReadonlySet<string>,
  pattern: string = "{name}",
): string {
  const base = applyBranchNamePattern(
    pattern,
    taskBranchSlug(prompt) || "task",
  );
  let name = base;
  for (let n = 2; taken.has(name); n++) name = `${base}-${n}`;
  return name;
}

/**
 * Creates a workspace for a task typed in the home view, through the same
 * `create_workspace` command the new-workspace dialog uses. Returns its id.
 */
export async function createTaskWorkspace(
  repoPath: string,
  prompt: string,
  title: string,
): Promise<number> {
  const [workspaces, branches, pattern] = await Promise.all([
    getWorkspaces(repoPath),
    listRepoBranches(repoPath),
    getRepoSetting(repoPath, "branch_name_pattern"),
  ]);
  const taken = new Set([
    ...workspaces.map((ws) => ws.branch_name),
    ...branches.map((branch) => branch.name),
  ]);
  return createWorkspace(
    repoPath,
    taskBranchName(prompt, taken, pattern || "treq/{name}"),
    undefined,
    buildCreateMetadata({ title, description: "", sparsePaths: "" }),
  );
}
