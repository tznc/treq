import type { Workspace } from "../lib/api";
import {
  type GpuiOverlayView,
  gpuiDrawsOverlays,
  useGpuiOverlay,
} from "../lib/gpui-overlay";
import type { GpuiScreenAction } from "../lib/gpui-screen";
import { dispatchGpuiAppShortcut } from "../lib/gpui-app-shortcut";
import { useChecksCommands } from "./checks/useChecksCommands";

/** Which of Dashboard's window-wide dialogs are open, and their setters. */
interface OverlayState {
  open: boolean;
  setOpen: (open: boolean) => void;
}

interface Params {
  /** GPUI may draw them (a local repository). */
  enabled: boolean;
  palette: OverlayState;
  shortcuts: OverlayState;
  workspacePicker: OverlayState;
  branchSwitcher: OverlayState;
  repoPath: string;
  stash: boolean;
  selectedWorkspace: Workspace | null;
  workspaces: Workspace[];
  /** The palette's Dashboard commands, by `CommandPalette` item id. */
  commands: Record<string, () => void>;
  onSelectWorkspace: (workspace: Workspace) => void;
  onBranchChanged: (branchName: string) => void;
}

/**
 * Draws Dashboard's `CommandPalette`, `KeyboardShortcutsModal`,
 * `WorkspacePicker` and `BranchSwitcher` in GPUI over the whole window, and
 * runs what GPUI sends back. Returns whether GPUI draws them, so React
 * leaves its own closed.
 */
export function useGpuiOverlays(p: Params): boolean {
  const gpui = p.enabled && gpuiDrawsOverlays();
  const checksCommands = useChecksCommands(p.selectedWorkspace);

  // The dialog opened last is on top in React; those open together are
  // stacked in this order.
  const shown: [OverlayState, GpuiOverlayView | null][] = [
    [
      p.workspacePicker,
      p.repoPath ? { kind: "workspacePicker", repoPath: p.repoPath } : null,
    ],
    [p.shortcuts, { kind: "keyboardShortcuts" }],
    [
      p.branchSwitcher,
      p.repoPath ? { kind: "branchSwitcher", repoPath: p.repoPath } : null,
    ],
    [
      p.palette,
      {
        kind: "commandPalette",
        repoPath: p.repoPath,
        stash: p.stash,
        hasSelectedWorkspace: !!p.selectedWorkspace,
        workspace: p.selectedWorkspace
          ? {
              id: p.selectedWorkspace.id,
              repoPath: p.selectedWorkspace.repo_path,
              branch: p.selectedWorkspace.branch_name,
            }
          : null,
      },
    ],
  ];
  const top = shown.find(([state, view]) => state.open && view);
  const view = gpui && top ? top[1] : null;

  const closePalette = () => p.palette.setOpen(false);

  const press = (id: string) => {
    if (id === "overlay.commandPalette") {
      p.palette.setOpen(true);
      return;
    }
    if (id === "overlay.close") {
      top?.[0].setOpen(false);
      return;
    }
    if (!id.startsWith("palette.")) return;
    const command = id.slice("palette.".length);
    const run =
      p.commands[command] ??
      checksCommands.find((c) => c.id === command)?.onSelect;
    run?.();
    closePalette();
  };

  const field = (name: string, value: unknown) => {
    switch (name) {
      case "overlay.shortcut":
        // Dashboard's shortcuts pressed while GPUI has focus.
        if (typeof value === "string") dispatchGpuiAppShortcut(value);
        return;
      case "palette.navigate":
        if (typeof value === "string" && value) window.location.href = value;
        closePalette();
        return;
      case "workspacePicker.select": {
        const workspace = p.workspaces.find((w) => w.id === value);
        if (workspace) p.onSelectWorkspace(workspace);
        p.workspacePicker.setOpen(false);
        return;
      }
      case "branchSwitcher.changed":
        p.onBranchChanged(String(value));
        p.branchSwitcher.setOpen(false);
        return;
      default:
    }
  };

  useGpuiOverlay(view, (action: GpuiScreenAction) => {
    if (action.kind === "press") press(action.id);
    else if (action.kind === "field") field(action.field, action.value);
  });
  return gpui;
}
