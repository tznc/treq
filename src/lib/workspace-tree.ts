import type { Workspace, WorkspaceSidebarStatus } from "./api";

/**
 * WorkspaceSidebarStatus plus git detail only populated for workspaces with
 * an active agent session (see the sidebar agent-session spinner).
 */
export type WorkspaceSidebarStatusWithAgentDetail = WorkspaceSidebarStatus & {
  has_changes?: boolean;
  commits_ahead_of_target_count?: number;
};

/**
 * Represents a node in the workspace tree
 */
export interface WorkspaceTreeNode {
  status: WorkspaceSidebarStatusWithAgentDetail;
  branchName: string;
  children: WorkspaceTreeNode[];
  depth: number;
}

/**
 * Flattened node for rendering
 */
export interface FlattenedWorkspaceNode {
  status: WorkspaceSidebarStatusWithAgentDetail;
  branchName: string;
  depth: number;
  hasChildren: boolean;
  parentBranch: string | null;
}

/**
 * Build a hierarchical tree from flat workspace list
 *
 * Algorithm:
 * 1. Create a map of branch_name → workspace for quick lookup
 * 2. For each workspace, find parent by matching its target_branch to another workspace's branch_name
 * 3. Build parent-child relationships
 * 4. Roots = workspaces whose target_branch has no matching workspace (or is null)
 * 5. Sort siblings by recency (newest activity first) at each level
 *
 * @param workspaces Flat list of workspaces
 * @returns Array of root nodes forming a forest
 */
export function buildWorkspaceTree(
  statuses: WorkspaceSidebarStatus[],
  isMergedBranch: (branchName: string) => boolean = () => false,
): WorkspaceTreeNode[] {
  const workspaces = statuses.map((statusEntry) => statusEntry.current);
  // Step 1: Create lookup map by branch_name
  const workspaceByBranch = new Map<string, Workspace>();
  for (const ws of workspaces) {
    workspaceByBranch.set(ws.branch_name, ws);
  }

  // Step 2: Create nodes for all workspaces
  const nodeByBranch = new Map<string, WorkspaceTreeNode>();
  for (const ws of workspaces) {
    nodeByBranch.set(ws.branch_name, {
      branchName: ws.branch_name,
      children: [],
      depth: 0,
      status: statuses.find((statusEntry) => statusEntry.current.id === ws.id)!,
    });
  }

  // Step 3: Build parent-child relationships
  for (const ws of workspaces) {
    const target = ws.target_branch;
    if (!target) continue; // No target = root node
    if (target === ws.branch_name) continue; // Ignore self-target links

    const parentNode = nodeByBranch.get(target);
    const childNode = nodeByBranch.get(ws.branch_name);

    if (parentNode && childNode) {
      // Valid parent-child relationship
      parentNode.children.push(childNode);
    }
    // If parent doesn't exist (targets external branch like origin/main), it becomes a root
  }

  // Step 4: Find root nodes (workspaces with no target or target not in workspace list)
  const roots: WorkspaceTreeNode[] = [];
  for (const node of nodeByBranch.values()) {
    const targetBranch = node.status.current.target_branch;
    const hasTarget = targetBranch !== null && targetBranch !== undefined;
    const isSelfTarget = hasTarget && targetBranch === node.branchName;
    const targetExists = hasTarget && workspaceByBranch.has(targetBranch);

    if (!hasTarget || !targetExists || isSelfTarget) {
      roots.push(node);
    }
  }

  // Malformed graphs can form cycles that no root reaches; flatten those
  // workspaces to top level so sidebar content never disappears.
  const reached = new Set<WorkspaceTreeNode>();
  const pending = [...roots];
  while (pending.length > 0) {
    const node = pending.pop()!;
    reached.add(node);
    pending.push(...node.children);
  }
  for (const node of nodeByBranch.values()) {
    if (!reached.has(node)) {
      node.children = [];
      roots.push(node);
    }
  }

  // Step 5: Compute depths and sort
  for (const root of roots) {
    computeDepths(root, 0);
  }
  sortTreeByRecency(roots, isMergedBranch);

  return roots;
}

/**
 * Recursively compute depths for all nodes in the tree
 */
function computeDepths(node: WorkspaceTreeNode, depth: number): void {
  node.depth = depth;
  for (const child of node.children) {
    computeDepths(child, depth + 1);
  }
}

/**
 * Activity timestamp used for sibling ordering. Prefer last_activity_at
 * (commit / WC tip) and fall back to created_at.
 */
function activityTimestamp(node: WorkspaceTreeNode): string {
  return node.status.last_activity_at || node.status.current.created_at || "";
}

/**
 * Sort tree nodes by recency at each sibling level (newest first), with
 * merged workspaces always sorted after unmerged ones regardless of
 * recency. Hierarchy is unchanged — only peer order within a parent (or
 * among roots). Equal timestamps fall back to branch name for stability.
 */
function sortTreeByRecency(
  nodes: WorkspaceTreeNode[],
  isMergedBranch: (branchName: string) => boolean,
): void {
  nodes.sort((nodeA, nodeB) => {
    const mergedA = isMergedBranch(nodeA.branchName);
    const mergedB = isMergedBranch(nodeB.branchName);
    if (mergedA !== mergedB) {
      return mergedA ? 1 : -1;
    }
    const timeA = activityTimestamp(nodeA);
    const timeB = activityTimestamp(nodeB);
    if (timeA !== timeB) {
      return timeB.localeCompare(timeA);
    }
    return nodeA.branchName.localeCompare(nodeB.branchName);
  });
  for (const node of nodes) {
    sortTreeByRecency(node.children, isMergedBranch);
  }
}

/**
 * Flatten tree for rendering (depth-first traversal)
 *
 * @param roots Array of root nodes
 * @returns Flattened list in display order with depth info
 */
export function flattenWorkspaceTree(
  roots: WorkspaceTreeNode[],
): FlattenedWorkspaceNode[] {
  const result: FlattenedWorkspaceNode[] = [];

  function traverse(node: WorkspaceTreeNode): void {
    result.push({
      branchName: node.branchName,
      depth: node.depth,
      hasChildren: node.children.length > 0,
      parentBranch: node.status.current.target_branch ?? null,
      status: node.status,
    });

    for (const child of node.children) {
      traverse(child);
    }
  }

  for (const root of roots) {
    traverse(root);
  }

  return result;
}

/**
 * Recursively collect all descendant workspaces of a given branch.
 * A workspace W is a descendant of branchName if W.target_branch === branchName,
 * or W.target_branch is itself a descendant.
 */
export function getDescendants(
  workspaces: Workspace[],
  branchName: string,
): Workspace[] {
  const childrenMap = new Map<string, Workspace[]>();
  for (const ws of workspaces) {
    if (ws.target_branch) {
      const list = childrenMap.get(ws.target_branch) || [];
      list.push(ws);
      childrenMap.set(ws.target_branch, list);
    }
  }

  const result: Workspace[] = [];
  const stack = [branchName];
  const visited = new Set<string>([branchName]);

  while (stack.length > 0) {
    const current = stack.pop()!;
    const children = childrenMap.get(current) || [];
    for (const child of children) {
      if (!visited.has(child.branch_name)) {
        visited.add(child.branch_name);
        result.push(child);
        stack.push(child.branch_name);
      }
    }
  }

  return result;
}

/**
 * Follow target_branch chain upward to find the root workspace of a stack.
 * The root is a workspace whose target_branch either doesn't exist in the
 * workspace list or is null (i.e., it targets an external branch like "main").
 *
 * Returns the branch_name of the root workspace.
 */
function getStackRoot(workspaces: Workspace[], branchName: string): string {
  const workspaceByBranch = new Map<string, Workspace>();
  for (const ws of workspaces) {
    workspaceByBranch.set(ws.branch_name, ws);
  }

  let current = branchName;
  const visited = new Set<string>();

  while (true) {
    visited.add(current);
    const ws = workspaceByBranch.get(current);
    if (!ws || !ws.target_branch) {
      return current;
    }
    // If target_branch is not a workspace (external branch), current is root
    if (!workspaceByBranch.has(ws.target_branch)) {
      return current;
    }
    // Cycle detection
    if (visited.has(ws.target_branch)) {
      return current;
    }
    current = ws.target_branch;
  }
}

/**
 * Get entire stack: root + all its descendants.
 * Finds the root via getStackRoot, then collects all descendants.
 */
export function getEntireStack(
  workspaces: Workspace[],
  branchName: string,
): Workspace[] {
  const rootBranch = getStackRoot(workspaces, branchName);
  const root = workspaces.find(
    (workspace) => workspace.branch_name === rootBranch,
  );
  if (!root) return [];

  const descendants = getDescendants(workspaces, rootBranch);
  return [root, ...descendants];
}

// Tree preview helpers for split/stack dialogs

export interface StackedWorkspaceEntry {
  workspace: Workspace;
  isCurrent: boolean;
}

/**
 * Walk `target_branch` toward the stack root and collect workspace ancestors
 * (parent first, root last). Stops at an external/missing target. Does not
 * include siblings of the starting workspace or of any ancestor.
 */
function getAncestorWorkspaces(
  workspaces: Workspace[],
  branchName: string,
): Workspace[] {
  const workspaceByBranch = new Map<string, Workspace>();
  for (const ws of workspaces) {
    workspaceByBranch.set(ws.branch_name, ws);
  }

  const ancestors: Workspace[] = [];
  const visited = new Set<string>([branchName]);
  let current = branchName;

  while (true) {
    const ws = workspaceByBranch.get(current);
    if (!ws?.target_branch) break;
    const parent = workspaceByBranch.get(ws.target_branch);
    if (!parent) break;
    if (visited.has(parent.branch_name)) break;
    visited.add(parent.branch_name);
    ancestors.push(parent);
    current = parent.branch_name;
  }

  return ancestors;
}

/**
 * Build the ordered stack (tip-first, root-last) for the Code tab stack card.
 *
 * Includes the current workspace, its ancestor chain (the target and
 * workspaces between here and the stack root), and all descendants of the
 * current workspace — including sibling forks among those descendants.
 * Siblings of the current workspace itself are omitted.
 *
 * Returns null when the workspace isn't part of a multi-workspace stack —
 * i.e. it is alone (no workspace ancestors or descendants), or the id
 * isn't found. A stack root with descendants still returns the descendant
 * tree so the Code tab stack card stays visible on the first workspace.
 */
export function getWorkspaceStack(
  workspaces: Workspace[],
  currentWorkspaceId: number,
): StackedWorkspaceEntry[] | null {
  const current = workspaces.find((ws) => ws.id === currentWorkspaceId);
  if (!current) return null;

  const descendants = getDescendants(workspaces, current.branch_name);
  const ancestors = getAncestorWorkspaces(workspaces, current.branch_name);
  // A single workspace targeting main/external isn't a stack worth showing.
  if (descendants.length === 0 && ancestors.length === 0) return null;

  const chain = [...ancestors].reverse().concat(current, descendants);

  return [...chain].reverse().map((workspace) => ({
    workspace,
    isCurrent: workspace.id === currentWorkspaceId,
  }));
}

export function getValidTargets(
  workspaces: Workspace[],
  currentBranch: string,
): string[] {
  const validTargets: string[] = [];

  for (const ws of workspaces) {
    // Can't target self — every other workspace is valid because
    // planWorkspaceTargetMove lifts the bridge node when the target
    // is a descendant (parent/child stack reorder).
    if (ws.branch_name === currentBranch) {
      continue;
    }

    validTargets.push(ws.branch_name);
  }

  return validTargets;
}

export type NewWorkspaceChainEntry =
  | { kind: "new" }
  | { kind: "branch"; branch: string; workspace: Workspace | null };

/**
 * Build the linear chain (tip-first) that a new workspace will join, for the
 * stack card in the create dialog. The chain runs from the tip down to, but
 * not including, `defaultBranch`. "after" puts the new workspace on top of
 * `parentBranch`; "before" inserts it between `parentBranch` and its target.
 */
export function buildNewWorkspaceChain(
  workspaces: Workspace[],
  {
    parentBranch,
    position,
    defaultBranch,
  }: {
    parentBranch: string;
    position: "before" | "after";
    defaultBranch: string;
  },
): NewWorkspaceChainEntry[] {
  const workspaceByBranch = new Map(
    workspaces.map((workspace) => [workspace.branch_name, workspace]),
  );

  const below: NewWorkspaceChainEntry[] = [];
  const visited = new Set<string>();
  let branch: string | null = parentBranch;
  while (branch && branch !== defaultBranch && !visited.has(branch)) {
    visited.add(branch);
    const workspace: Workspace | null = workspaceByBranch.get(branch) ?? null;
    below.push({ kind: "branch", branch, workspace });
    // A branch that is not a workspace yet sits on the default branch.
    branch = workspace?.target_branch ?? null;
  }

  const newEntry: NewWorkspaceChainEntry = { kind: "new" };
  if (position === "before" && below.length > 0) {
    return [below[0], newEntry, ...below.slice(1)];
  }
  return [newEntry, ...below];
}
