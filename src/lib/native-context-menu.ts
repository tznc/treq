import { Menu } from "@tauri-apps/api/menu";
import type { MenuItemOptions } from "@tauri-apps/api/menu/menuItem";
import type { PredefinedMenuItemOptions } from "@tauri-apps/api/menu/predefinedMenuItem";
import type { SubmenuOptions } from "@tauri-apps/api/menu/submenu";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import type { Workspace } from "./api";
import { copyTextToClipboard, getFullWorkspacePath } from "./utils";

/**
 * A context menu entry, described as data so menus can be built and tested
 * without the native menu API.
 */
export type NativeMenuEntry =
  | { kind: "item"; text: string; onSelect: () => void }
  | { kind: "separator" }
  | { kind: "submenu"; text: string; items: NativeMenuEntry[] };

/** Installed editors, from the editor apps store. */
export interface EditorApps {
  cursor: boolean;
  vscode: boolean;
  zed: boolean;
}

const separator: NativeMenuEntry = { kind: "separator" };

function item(text: string, onSelect: () => void): NativeMenuEntry {
  return { kind: "item", text, onSelect };
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
    item("Open in Finder", () => {
      revealItemInDir(fullPath).catch((error) =>
        console.error("Failed to reveal path:", error),
      );
    }),
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
    item("Copy relative path", () => copy(relativePath)),
    item("Copy full path", () => copy(fullPath)),
    { kind: "submenu", text: "Open in...", items: openInItems },
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
    entries.push(item("Switch Branch...", params.onOpenBranchSwitcher));
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
    repoItems.push(item("Add Repository…", params.onAddRepository));
  }
  if (params.onOpenRepoSettings) {
    repoItems.push(item("Repository Settings", params.onOpenRepoSettings));
  }
  if (params.onLocateRepo) {
    repoItems.push(item("Locate…", params.onLocateRepo));
  }
  if (params.onRemoveRepo) {
    repoItems.push(item("Remove Repository", params.onRemoveRepo));
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
    item("Copy branch name", () => copy(title)),
    ...(prUrl ? [item("Copy link to GitHub PR", () => copy(prUrl))] : []),
    item("Rename Workspace", params.onRename),
    separator,
    ...pathMenuEntries({
      relativePath: workspaceRelativePath(workspace, params.repoPath),
      fullPath: getFullWorkspacePath(workspace),
      editorApps: params.editorApps,
    }),
    ...(params.onArchive
      ? [separator, item("Archive Workspace", params.onArchive)]
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

/** Pop a native context menu at the pointer in the current window. */
export async function popupNativeMenu(
  entries: NativeMenuEntry[],
): Promise<void> {
  try {
    const menu = await Menu.new({ items: toMenuOptions(entries) });
    await menu.popup();
  } catch (error) {
    console.error("Failed to open the context menu:", error);
  }
}
