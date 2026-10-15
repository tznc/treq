import { describe, expect, it, vi } from "vitest";
import { createMockWorkspace } from "../../test/factories/workspace.factory";
import {
  homeMenuEntries,
  type NativeMenuEntry,
  pathMenuEntries,
  workspaceMenuEntries,
  workspaceRelativePath,
} from "./native-context-menu";

const noEditors = { cursor: false, vscode: false, zed: false };

/** Menu labels, with "---" for separators and submenus as nested lists. */
function labels(entries: NativeMenuEntry[]): unknown[] {
  return entries.map((entry) => {
    if (entry.kind === "separator") return "---";
    if (entry.kind === "submenu") return { [entry.text]: labels(entry.items) };
    return entry.text;
  });
}

describe("pathMenuEntries", () => {
  it("lists only the editors that are installed", () => {
    const entries = pathMenuEntries({
      relativePath: ".",
      fullPath: "/r",
      editorApps: { cursor: true, vscode: false, zed: true },
    });
    expect(labels(entries)).toEqual([
      "Copy relative path",
      "Copy full path",
      { "Open in...": ["Open in Finder", "Open in Cursor", "Open in Zed"] },
    ]);
  });
});

describe("homeMenuEntries", () => {
  it("matches the React home row menu", () => {
    const entries = homeMenuEntries({
      repoPath: "/r",
      editorApps: noEditors,
      onOpenBranchSwitcher: vi.fn(),
      onAddRepository: vi.fn(),
    });
    expect(labels(entries)).toEqual([
      "Switch Branch...",
      "---",
      "Copy relative path",
      "Copy full path",
      { "Open in...": ["Open in Finder"] },
      "---",
      "Add Repository…",
    ]);
  });

  it("drops path items for a missing repository and items without handlers", () => {
    const entries = homeMenuEntries({
      repoPath: "/r",
      missing: true,
      editorApps: noEditors,
      onLocateRepo: vi.fn(),
      onRemoveRepo: vi.fn(),
    });
    expect(labels(entries)).toEqual(["---", "Locate…", "Remove Repository"]);
  });

  it("runs the handler of the chosen item", () => {
    const onOpenBranchSwitcher = vi.fn();
    const [first] = homeMenuEntries({
      repoPath: "/r",
      editorApps: noEditors,
      onOpenBranchSwitcher,
    });
    if (first.kind !== "item") throw new Error("expected an item");
    first.onSelect();
    expect(onOpenBranchSwitcher).toHaveBeenCalledOnce();
  });
});

describe("workspaceMenuEntries", () => {
  const workspace = createMockWorkspace({
    repo_path: "/r",
    workspace_path: "feat-a",
    branch_name: "feat/a",
  });

  it("matches the React workspace row menu", () => {
    const entries = workspaceMenuEntries({
      workspace,
      title: "feat/a",
      repoPath: "/r",
      prUrl: "https://github.com/o/r/pull/1",
      editorApps: noEditors,
      onRename: vi.fn(),
      onArchive: vi.fn(),
    });
    expect(labels(entries)).toEqual([
      "Copy branch name",
      "Copy link to GitHub PR",
      "Rename Workspace",
      "---",
      "Copy relative path",
      "Copy full path",
      { "Open in...": ["Open in Finder"] },
      "---",
      "Archive Workspace",
    ]);
  });

  it("leaves out the PR link without a PR", () => {
    const entries = workspaceMenuEntries({
      workspace,
      title: "feat/a",
      repoPath: "/r",
      prUrl: null,
      editorApps: noEditors,
      onRename: vi.fn(),
      onArchive: vi.fn(),
    });
    expect(labels(entries)).not.toContain("Copy link to GitHub PR");
  });

  it("finds a workspace's path relative to its repository", () => {
    expect(workspaceRelativePath(workspace, "/r")).toBe(
      ".treq/workspaces/feat-a",
    );
    expect(
      workspaceRelativePath({ ...workspace, workspace_path: "/r/wt/a" }, "/r"),
    ).toBe("wt/a");
    expect(
      workspaceRelativePath({ ...workspace, workspace_path: "/x/a" }, "/r"),
    ).toBe("/x/a");
  });
});
