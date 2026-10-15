import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { MenuItemOptions } from "@tauri-apps/api/menu/menuItem";
import type { PredefinedMenuItemOptions } from "@tauri-apps/api/menu/predefinedMenuItem";
import type { SubmenuOptions } from "@tauri-apps/api/menu/submenu";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { gpuiMenuHide, gpuiMenuShow, type Workspace } from "./api";
import { isGpuiShellHost } from "./gpui-shell";
import { copyTextToClipboard, getFullWorkspacePath } from "./utils";

/**
 * A context menu entry, described as data so menus can be built and tested
 * without the native menu API.
 */
export type NativeMenuEntry =
  | {
      kind: "item";
      text: string;
      onSelect: () => void;
      icon?: MenuIcon;
      destructive?: boolean;
    }
  | { kind: "separator" }
  | {
      kind: "submenu";
      text: string;
      items: NativeMenuEntry[];
      icon?: MenuIcon;
    };

/** The lucide icon beside an item, as the React context menus drew them. */
export type MenuIcon =
  | "archive"
  | "copy"
  | "folder-open"
  | "folder-plus"
  | "folder-search"
  | "git-branch"
  | "link"
  | "pencil"
  | "settings"
  | "trash-2";

/** Installed editors, from the editor apps store. */
export interface EditorApps {
  cursor: boolean;
  vscode: boolean;
  zed: boolean;
}

const separator: NativeMenuEntry = { kind: "separator" };

function item(
  text: string,
  onSelect: () => void,
  style: { icon?: MenuIcon; destructive?: boolean } = {},
): NativeMenuEntry {
  return { kind: "item", text, onSelect, ...style };
}

function copy(text: string) {
  copyTextToClipboard(text).catch((error) =>
    console.error("Failed to copy to the clipboard:", error),
  );
}

function openIn(app: string, url: string) {
  openUrl(url).catch((error) =>
    console.error(`Failed to open in ${app}:`, error),
  );
}

/** Copy and "Open in…" items for a path, as the React sidebar menus have. */
export function pathMenuEntries(params: {
  relativePath: string;
  fullPath: string;
  editorApps: EditorApps;
}): NativeMenuEntry[] {
  const { relativePath, fullPath, editorApps } = params;
  const openInItems: NativeMenuEntry[] = [
    item(
      "Open in Finder",
      () => {
        revealItemInDir(fullPath).catch((error) =>
          console.error("Failed to reveal path:", error),
        );
      },
      { icon: "folder-open" },
    ),
  ];
  if (editorApps.cursor) {
    openInItems.push(
      item("Open in Cursor", () =>
        openIn("Cursor", `cursor://file/${fullPath}`),
      ),
    );
  }
  if (editorApps.vscode) {
    openInItems.push(
      item("Open in VSCode", () =>
        openIn("VSCode", `vscode://file/${fullPath}`),
      ),
    );
  }
  if (editorApps.zed) {
    openInItems.push(
      item("Open in Zed", () => openIn("Zed", `zed://file/${fullPath}`)),
    );
  }
  return [
    item("Copy relative path", () => copy(relativePath), { icon: "copy" }),
    item("Copy full path", () => copy(fullPath), { icon: "copy" }),
    {
      kind: "submenu",
      text: "Open in...",
      items: openInItems,
      icon: "folder-open",
    },
  ];
}

/** The home row's menu. Handlers left out drop their items. */
export function homeMenuEntries(params: {
  repoPath: string;
  missing?: boolean;
  editorApps: EditorApps;
  onOpenBranchSwitcher?: () => void;
  onAddRepository?: () => void;
  onOpenRepoSettings?: () => void;
  onLocateRepo?: () => void;
  onRemoveRepo?: () => void;
}): NativeMenuEntry[] {
  const entries: NativeMenuEntry[] = [];
  if (params.onOpenBranchSwitcher) {
    entries.push(
      item("Switch Branch...", params.onOpenBranchSwitcher, {
        icon: "git-branch",
      }),
    );
    entries.push(separator);
  }
  if (!params.missing) {
    entries.push(
      ...pathMenuEntries({
        relativePath: ".",
        fullPath: params.repoPath,
        editorApps: params.editorApps,
      }),
    );
  }
  const repoItems: NativeMenuEntry[] = [];
  if (params.onAddRepository) {
    repoItems.push(
      item("Add Repository…", params.onAddRepository, { icon: "folder-plus" }),
    );
  }
  if (params.onOpenRepoSettings) {
    repoItems.push(
      item("Repository Settings", params.onOpenRepoSettings, {
        icon: "settings",
      }),
    );
  }
  if (params.onLocateRepo) {
    repoItems.push(
      item("Locate…", params.onLocateRepo, { icon: "folder-search" }),
    );
  }
  if (params.onRemoveRepo) {
    repoItems.push(
      item("Remove Repository", params.onRemoveRepo, {
        icon: "trash-2",
        destructive: true,
      }),
    );
  }
  if (repoItems.length > 0) entries.push(separator, ...repoItems);
  return entries;
}

/** Where a workspace lives, relative to its repository. */
export function workspaceRelativePath(
  workspace: Workspace,
  repoPath: string | undefined,
): string {
  if (!workspace.workspace_path.startsWith("/")) {
    return `.treq/workspaces/${workspace.workspace_path}`;
  }
  return repoPath && workspace.workspace_path.startsWith(repoPath)
    ? workspace.workspace_path.slice(repoPath.length + 1)
    : workspace.workspace_path;
}

/** A workspace row's menu. */
export function workspaceMenuEntries(params: {
  workspace: Workspace;
  title: string;
  repoPath: string | undefined;
  prUrl: string | null;
  editorApps: EditorApps;
  onRename: () => void;
  /** Undefined for a repository that is not on screen. */
  onArchive?: () => void;
}): NativeMenuEntry[] {
  const { workspace, title, prUrl } = params;
  return [
    item("Copy branch name", () => copy(title), { icon: "git-branch" }),
    ...(prUrl
      ? [item("Copy link to GitHub PR", () => copy(prUrl), { icon: "link" })]
      : []),
    item("Rename Workspace", params.onRename, { icon: "pencil" }),
    separator,
    ...pathMenuEntries({
      relativePath: workspaceRelativePath(workspace, params.repoPath),
      fullPath: getFullWorkspacePath(workspace),
      editorApps: params.editorApps,
    }),
    ...(params.onArchive
      ? [
          separator,
          item("Archive Workspace", params.onArchive, {
            icon: "archive",
            destructive: true,
          }),
        ]
      : []),
  ];
}

type MenuOptionsItem =
  | MenuItemOptions
  | SubmenuOptions
  | PredefinedMenuItemOptions;

function toMenuOptions(entries: NativeMenuEntry[]): MenuOptionsItem[] {
  return entries.map((entry): MenuOptionsItem => {
    switch (entry.kind) {
      case "separator":
        return { item: "Separator" };
      case "submenu":
        return { text: entry.text, items: toMenuOptions(entry.items) };
      default:
        return { text: entry.text, action: () => entry.onSelect() };
    }
  });
}

/** A menu entry as the GPUI shell's `gpui_menu_show` takes it. */
export type GpuiMenuEntry =
  | {
      kind: "item";
      id: number;
      text: string;
      icon?: MenuIcon;
      destructive?: boolean;
    }
  | { kind: "separator" }
  | { kind: "submenu"; text: string; icon?: MenuIcon; items: GpuiMenuEntry[] };

/**
 * Numbers `entries`' items for the GPUI shell, collecting each item's
 * handler under its id.
 */
export function toGpuiMenu(entries: NativeMenuEntry[]): {
  entries: GpuiMenuEntry[];
  handlers: Map<number, () => void>;
} {
  const handlers = new Map<number, () => void>();
  const convert = (list: NativeMenuEntry[]): GpuiMenuEntry[] =>
    list.map((entry): GpuiMenuEntry => {
      switch (entry.kind) {
        case "separator":
          return entry;
        case "submenu":
          return {
            kind: "submenu",
            text: entry.text,
            icon: entry.icon,
            items: convert(entry.items),
          };
        default: {
          const id = handlers.size;
          handlers.set(id, entry.onSelect);
          return {
            kind: "item",
            id,
            text: entry.text,
            icon: entry.icon,
            destructive: entry.destructive,
          };
        }
      }
    });
  return { entries: convert(entries), handlers };
}

const GPUI_MENU_SELECT_EVENT = "gpui-menu-select";

/** Handlers of the open GPUI menu's items; empty with no menu open. */
let gpuiMenuHandlers = new Map<number, () => void>();
let gpuiMenuListening = false;

function hideGpuiMenu() {
  if (gpuiMenuHandlers.size === 0) return;
  gpuiMenuHandlers = new Map();
  gpuiMenuHide().catch((error) =>
    console.error("Failed to close the menu:", error),
  );
}

/** Runs picks from GPUI menus, and closes them on a click or Escape here. */
function listenGpuiMenus() {
  if (gpuiMenuListening) return;
  gpuiMenuListening = true;
  getCurrentWebviewWindow()
    .listen<{ id: number }>(GPUI_MENU_SELECT_EVENT, (event) => {
      const handler = gpuiMenuHandlers.get(event.payload.id);
      gpuiMenuHandlers = new Map();
      handler?.();
    })
    .catch((error) => console.error("Failed to listen for menus:", error));
  window.addEventListener("pointerdown", hideGpuiMenu, true);
  window.addEventListener(
    "keydown",
    (event) => {
      if (event.key === "Escape") hideGpuiMenu();
    },
    true,
  );
}

/**
 * Pop a context menu at the pointer in the current window: drawn by GPUI,
 * styled as the React menus were, under the GPUI shell; native otherwise.
 */
export async function popupNativeMenu(
  entries: NativeMenuEntry[],
): Promise<void> {
  if (isGpuiShellHost()) {
    listenGpuiMenus();
    const menu = toGpuiMenu(entries);
    gpuiMenuHandlers = menu.handlers;
    try {
      await gpuiMenuShow(menu.entries);
    } catch (error) {
      console.error("Failed to open the context menu:", error);
    }
    return;
  }
  try {
    const menu = await Menu.new({ items: toMenuOptions(entries) });
    await menu.popup();
  } catch (error) {
    console.error("Failed to open the context menu:", error);
  }
}
