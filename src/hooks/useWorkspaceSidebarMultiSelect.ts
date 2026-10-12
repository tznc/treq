import { useRef } from "react";
import type { Workspace } from "../lib/api";
import type { FlattenedWorkspaceNode } from "../lib/workspace-tree";

const heldModifiers = {
  shift: false,
  meta: false,
  ctrl: false,
};

let modifierListenersBound = false;

function bindModifierKeyListeners() {
  if (modifierListenersBound || typeof window === "undefined") return;
  modifierListenersBound = true;
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Shift") heldModifiers.shift = true;
    if (event.key === "Meta") heldModifiers.meta = true;
    if (event.key === "Control") heldModifiers.ctrl = true;
  };
  const onKeyUp = (event: KeyboardEvent) => {
    if (event.key === "Shift") heldModifiers.shift = false;
    if (event.key === "Meta") heldModifiers.meta = false;
    if (event.key === "Control") heldModifiers.ctrl = false;
  };
  const onBlur = () => {
    heldModifiers.shift = false;
    heldModifiers.meta = false;
    heldModifiers.ctrl = false;
  };
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  window.addEventListener("blur", onBlur);
}

bindModifierKeyListeners();

function isModifierMouseEvent(
  event: React.MouseEvent | React.PointerEvent,
): boolean {
  return (
    event.shiftKey ||
    event.metaKey ||
    event.ctrlKey ||
    event.getModifierState("Shift") ||
    event.getModifierState("Meta") ||
    event.getModifierState("Control") ||
    heldModifiers.shift ||
    heldModifiers.meta ||
    heldModifiers.ctrl
  );
}

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

export function useWorkspaceSidebarMultiSelect({
  flattenedNodes,
  onSelectStack,
  onWorkspaceMultiSelect,
  onWorkspaceClick,
}: {
  flattenedNodes: FlattenedWorkspaceNode[];
  onSelectStack?: (workspaceIds: Set<number>) => void;
  onWorkspaceMultiSelect?: (
    workspace: Workspace | null,
    event: React.MouseEvent,
  ) => void;
  onWorkspaceClick?: (workspace: Workspace) => void;
}) {
  const lastSelectedIndexRef = useRef<number | null>(null);

  const clearLastSelectedIndex = () => {
    lastSelectedIndexRef.current = null;
  };

  const handleItemSelect = (
    workspace: Workspace,
    event: React.MouseEvent | React.PointerEvent,
    index: number,
  ) => {
    if (
      event.shiftKey ||
      event.getModifierState("Shift") ||
      heldModifiers.shift
    ) {
      if (lastSelectedIndexRef.current !== null && onSelectStack) {
        onSelectStack(
          idsInVisibleRange(
            flattenedNodes,
            lastSelectedIndexRef.current,
            index,
          ),
        );
        return;
      }
    }
    lastSelectedIndexRef.current = index;
    if (onWorkspaceMultiSelect) {
      onWorkspaceMultiSelect(workspace, event);
      return;
    }
    onWorkspaceClick?.(workspace);
  };

  return { handleItemSelect, clearLastSelectedIndex };
}

export function useWorkspaceRowPointerHandlers({
  onSelect,
}: {
  onSelect: (event: React.MouseEvent | React.PointerEvent) => void;
}) {
  const skipClickRef = useRef(false);

  const onPointerDown = (event: React.PointerEvent) => {
    if (event.button !== 0) return;
    if (!isModifierMouseEvent(event)) return;
    skipClickRef.current = true;
    onSelect(event);
  };

  const onClick = (event: React.MouseEvent) => {
    if (skipClickRef.current) {
      skipClickRef.current = false;
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    onSelect(event);
  };

  return { onPointerDown, onClick };
}
