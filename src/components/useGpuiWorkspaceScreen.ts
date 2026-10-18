import { openUrl } from "@tauri-apps/plugin-opener";
import type { useGpuiPartsHost } from "../lib/gpui-parts";
import {
  type GpuiScreenAction,
  type GpuiWorkspaceHeader,
  type GpuiWorkspaceScreen,
  type GpuiWorkspaceTab,
  type GpuiWorkspaceTabRow,
  useGpuiScreen,
} from "../lib/gpui-screen";
import { popupNativeMenu } from "../lib/native-context-menu";

/** The header controls drawn from a sub-component's own state (`useGpuiPart`). */
type PartControl = "target" | "viewPr" | "createPr" | "checks" | "mergeQueue";

type OwnHeader = Omit<GpuiWorkspaceHeader, PartControl> & {
  /** The sub-components ShowWorkspace renders, so their parts apply. */
  shows: Record<PartControl, boolean>;
};

interface Handlers {
  setTab: (tab: GpuiWorkspaceTab) => void;
  openBranchSwitcher?: () => void;
  homeRebase: () => void;
  stack?: () => void;
  push: () => void;
  sync: () => void;
  merge?: () => void;
  openDetails: () => void;
  deleteWorkspace?: () => void;
}

const TABS: GpuiWorkspaceTab[] = ["overview", "commits", "changes", "logs"];

type PartData<K extends PartControl> = Exclude<
  GpuiWorkspaceHeader[K],
  null
> & { open?: boolean };

/**
 * Draws ShowWorkspace's header and tab row in GPUI while `active`, with the
 * webview framed below them. Returns whether GPUI draws them, so React leaves
 * its own out of the frame.
 *
 * An open popover (target branch, checks, details) is React's: the screen
 * steps aside until it closes, so React draws the header with it.
 */
export function useGpuiWorkspaceScreen({
  active,
  parts,
  header,
  detailsOpen,
  tabRow,
  handlers,
}: {
  active: boolean;
  parts: ReturnType<typeof useGpuiPartsHost>;
  header: OwnHeader;
  detailsOpen: boolean;
  tabRow: GpuiWorkspaceTabRow | null;
  handlers: Handlers;
}): boolean {
  const { shows, ...own } = header;
  const part = <K extends PartControl>(key: K): PartData<K> | null =>
    (shows[key] && (parts.parts[key] as PartData<K> | null)) || null;
  const target = part("target");
  const checks = part("checks");
  const popoverOpen = detailsOpen || !!target?.open || !!checks?.open;

  const view: GpuiWorkspaceScreen = {
    kind: "workspace",
    header: {
      ...own,
      target: target && {
        branch: target.branch,
        loading: target.loading,
        disabled: target.disabled,
      },
      viewPr: part("viewPr"),
      createPr: part("createPr"),
      checks: checks && {
        status: checks.status,
        treq: checks.treq,
        github: checks.github,
      },
      mergeQueue: part("mergeQueue"),
    },
    tabRow,
  };

  const call = (key: PartControl, name: string) =>
    (parts.handlers(key)?.[name] as (() => void) | undefined)?.();

  const press = (id: string) => {
    switch (id) {
      case "workspace.branchSwitcher":
        return handlers.openBranchSwitcher?.();
      case "workspace.target":
        return call("target", "open");
      case "workspace.homeRebase":
        return handlers.homeRebase();
      case "workspace.stack":
        return handlers.stack?.();
      case "workspace.push":
        return handlers.push();
      case "workspace.viewPr":
        return call("viewPr", "view");
      case "workspace.viewPrWeb":
        return call("viewPr", "web");
      case "workspace.createPr":
        return call("createPr", "create");
      case "workspace.createPrMenu":
        return call("createPr", "menu");
      case "workspace.sync":
        return handlers.sync();
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
        case "openUrl":
          void openUrl(action.url);
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
