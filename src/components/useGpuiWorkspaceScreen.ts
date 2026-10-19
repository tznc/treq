import { useRunningCheckFiles } from "../hooks/useWorkspaceChecks";
import type { useGpuiPartsHost } from "../lib/gpui-parts";
import {
  type GpuiScreenAction,
  type GpuiWorkspaceScreen,
  type GpuiWorkspaceTab,
  useGpuiScreen,
} from "../lib/gpui-screen";
import { popupNativeMenu } from "../lib/native-context-menu";
import { useCacheRevision } from "../lib/swr-cache";

/** The header controls whose React flows GPUI still asks for (`useGpuiPart`). */
type PartControl = "target" | "viewPr" | "createPr" | "checks" | "mergeQueue";

type Route = Omit<
  GpuiWorkspaceScreen,
  "kind" | "creatingPr" | "runningChecks" | "mergeQueue" | "revision"
>;

interface Handlers {
  setTab: (tab: GpuiWorkspaceTab) => void;
  openBranchSwitcher?: () => void;
  stack?: () => void;
  merge?: () => void;
  openDetails: () => void;
  deleteWorkspace?: () => void;
}

const TABS: GpuiWorkspaceTab[] = ["overview", "commits", "changes", "logs"];

interface PartState {
  open?: boolean;
  creating?: boolean;
}

/**
 * Draws ShowWorkspace's header and tab row in GPUI while `active`, with the
 * webview framed below them. GPUI reads them from the backend and pushes,
 * syncs and rebases itself; React sends which workspace shows and the state
 * of the flows it still runs. Returns whether GPUI draws them, so React
 * leaves its own out of the frame.
 *
 * An open popover (target branch, checks, details) is React's: the screen
 * steps aside until it closes, so React draws the header with it.
 */
export function useGpuiWorkspaceScreen({
  active,
  parts,
  route,
  detailsOpen,
  handlers,
}: {
  active: boolean;
  parts: ReturnType<typeof useGpuiPartsHost>;
  route: Route;
  detailsOpen: boolean;
  handlers: Handlers;
}): boolean {
  const revision = useCacheRevision(active);
  const runningChecks = useRunningCheckFiles(route.workspaceId);
  const part = (key: PartControl) =>
    (parts.parts[key] as PartState | null | undefined) ?? null;
  const popoverOpen =
    detailsOpen || !!part("target")?.open || !!part("checks")?.open;

  const view: GpuiWorkspaceScreen = {
    kind: "workspace",
    ...route,
    creatingPr: !!part("createPr")?.creating,
    runningChecks,
    mergeQueue:
      (parts.parts.mergeQueue as GpuiWorkspaceScreen["mergeQueue"]) ?? null,
    revision,
  };

  const call = (key: PartControl, name: string) =>
    (parts.handlers(key)?.[name] as (() => void) | undefined)?.();

  const press = (id: string) => {
    switch (id) {
      case "workspace.branchSwitcher":
        return handlers.openBranchSwitcher?.();
      case "workspace.target":
        return call("target", "open");
      case "workspace.stack":
        return handlers.stack?.();
      case "workspace.viewPr":
        return call("viewPr", "view");
      case "workspace.createPr":
        return call("createPr", "create");
      case "workspace.createPrMenu":
        return call("createPr", "menu");
      case "workspace.checks":
        return call("checks", "open");
      case "workspace.mergeQueue":
        return call("mergeQueue", "press");
      case "workspace.merge":
        return handlers.merge?.();
      case "workspace.details":
        return handlers.openDetails();
      case "workspace.more": {
        const remove = handlers.deleteWorkspace;
        if (!remove) return;
        return void popupNativeMenu([
          {
            kind: "item",
            text: "Delete Workspace",
            icon: "trash-2",
            destructive: true,
            onSelect: remove,
          },
        ]);
      }
      default:
    }
  };

  return useGpuiScreen(
    active && !popoverOpen ? view : null,
    (action: GpuiScreenAction) => {
      if (!active) return;
      switch (action.kind) {
        case "press":
          press(action.id);
          break;
        case "field":
          if (
            action.field === "workspace.tab" &&
            TABS.includes(action.value as GpuiWorkspaceTab)
          ) {
            handlers.setTab(action.value as GpuiWorkspaceTab);
          }
          break;
        default:
      }
    },
  );
}
