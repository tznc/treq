import type { Workspace } from "./api";

/**
 * Get display title for a workspace - always returns branch_name
 */
export function getWorkspaceTitle(
  workspace: Workspace | { metadata?: string; branch_name: string },
): string {
  return workspace.branch_name;
}

/**
 * Get the human-facing display title for a workspace: explicit title, then
 * metadata.title, then branch_name as a last resort.
 */
export function getWorkspaceDisplayTitle(workspace: Workspace): string {
  if (workspace.title) return workspace.title;

  if (workspace.metadata) {
    try {
      const parsed = JSON.parse(workspace.metadata) as { title?: string };
      if (parsed.title) return parsed.title;
    } catch {
      // Malformed metadata falls through to branch_name below.
    }
  }

  return workspace.branch_name;
}
