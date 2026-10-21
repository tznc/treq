import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { REFRESH_WORKSPACE_CHANGES_EVENT } from "../lib/change-file-drag";
import type { WorkspaceChangesRefreshDetail } from "../lib/workspace-refresh";

/**
 * A counter bumped when `workspaceId`'s files change on disk or the window
 * regains focus, the two times `useFileLoading` re-reads the Changes tab, so
 * GPUI's Changes tab reads again too.
 */
export function useChangesRevision(
  workspaceId: number | undefined,
  active: boolean,
): number {
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    if (!active) return;
    const bump = () => setRevision((r) => r + 1);
    const handler = (event: Event) => {
      const { detail } = event as CustomEvent<WorkspaceChangesRefreshDetail>;
      if (
        detail?.workspaceId !== undefined &&
        workspaceId !== undefined &&
        detail.workspaceId !== workspaceId
      ) {
        return;
      }
      bump();
    };
    window.addEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, handler);
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) bump();
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        /* not running inside a Tauri window (e.g. tests) */
      });
    return () => {
      window.removeEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, handler);
      cancelled = true;
      unlisten?.();
    };
  }, [workspaceId, active]);

  return revision;
}
