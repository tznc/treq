import type { FlattenedWorkspaceNode } from "../lib/workspace-tree";

export function idsInVisibleRange(
  nodes: FlattenedWorkspaceNode[],
  fromIndex: number,
  toIndex: number,
): Set<number> {
  const start = Math.min(fromIndex, toIndex);
  const end = Math.max(fromIndex, toIndex);
  const ids = new Set<number>();
  for (let i = start; i <= end; i++) {
    ids.add(nodes[i].status.current.id);
  }
  return ids;
}
