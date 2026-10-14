// @include-parallel
import { beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import path from "path";
import { fireEvent, render, screen, waitFor, within } from "../../test-utils";
import userEvent from "@testing-library/user-event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  type Workspace,
  createWorkspace,
  getWorkspaces,
} from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import { dispatchRefreshWorkspaceChanges } from "../../../src/lib/change-file-drag";
import { clickGpuiRow } from "../../gpui-sidebar";

async function setupWorkspace(
  branchName: string,
  files: Record<string, string>,
): Promise<{
  repoPath: string;
  workspace: Workspace;
  workspacePath: string;
}> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  if (!workspace) {
    throw new Error(`Workspace not found for id ${workspaceId}`);
  }

  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );
  for (const [relativePath, content] of Object.entries(files)) {
    writeWorkspaceFile(workspacePath, relativePath, content);
  }

  return { repoPath, workspace, workspacePath };
}

async function openWorkspaceCodeBrowser(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
  fileName: string,
) {
  render(<Dashboard />);
  await clickGpuiRow(branchName);
  await user.click(await screen.findByRole("button", { name: fileName }));
  const fileBrowser = await screen.findByTestId("file-browser");
  const scoped = within(fileBrowser);
  await waitFor(() => {
    const hasCodeLines = scoped.queryAllByTestId("code-line").length > 0;
    const hasMarkdownTabs = scoped.queryByRole("tab", { name: "Code" });
    const failed = scoped.queryByText(/failed to load file content/i);
    expect(hasCodeLines || hasMarkdownTabs || failed).toBeTruthy();
  });
  return scoped;
}

describe("Dashboard - FileBrowser integration", { timeout: 15_000 }, () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup({ writeToClipboard: true });
    vi.clearAllMocks();
    vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
  });

  it("opens a selected file in the code browser and renders its contents", async () => {
    await setupWorkspace("feat/filebrowser-open-test", {
      "app.ts": "export function runApp() {\n  return 'ready';\n}\n",
      "src/helper.ts": "export const helper = true;\n",
    });

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-open-test",
      "app.ts",
    );

    const appTsLabels = await fileBrowser.findAllByText("app.ts");
    expect(appTsLabels.length).toBeGreaterThan(1);
    await fileBrowser.findByText("runApp");
    await screen.findByRole("button", { name: /back/i });
  });

  it("hides hidden directories from the directory tree", async () => {
    await setupWorkspace("feat/filebrowser-hidden-directories-test", {
      "app.ts": "export const app = true;\n",
      ".secret/config.json": "{}\n",
      ".env": "VISIBLE_FILE=true\n",
    });

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-hidden-directories-test",
      "app.ts",
    );

    expect(fileBrowser.queryByText(".jj")).toBeNull();
    expect(fileBrowser.queryByText(".git")).toBeNull();
    expect(fileBrowser.queryByText(".secret")).toBeNull();
    expect(await fileBrowser.findByText(".env")).toBeTruthy();
  });

  it("reloads the open file and directory tree after a scoped filesystem refresh", async () => {
    const { workspace, workspacePath } = await setupWorkspace(
      "feat/filebrowser-refresh-test",
      { "app.ts": "export const before = true;\n" },
    );

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-refresh-test",
      "app.ts",
    );
    await fileBrowser.findByText("before");

    writeWorkspaceFile(workspacePath, "app.ts", "export const after = true;\n");
    writeWorkspaceFile(
      workspacePath,
      "new-file.ts",
      "export const added = 1;\n",
    );
    dispatchRefreshWorkspaceChanges({
      workspaceId: workspace.id,
      changedPaths: [`${workspacePath}/app.ts`, `${workspacePath}/new-file.ts`],
    });

    await fileBrowser.findByText("after");
    expect(fileBrowser.queryByText("before")).toBeNull();
    await fileBrowser.findByText("new-file.ts");
  });

  it("uses Code and Preview tabs for markdown files and defaults to Code", async () => {
    await setupWorkspace("feat/filebrowser-markdown-preview-test", {
      "guide.md": "# Getting started\n\nUse **Treq** to ship changes.\n",
    });

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-markdown-preview-test",
      "guide.md",
    );

    const codeTab = await fileBrowser.findByRole("tab", { name: "Code" });
    const previewTab = await fileBrowser.findByRole("tab", { name: "Preview" });
    expect(codeTab).toHaveAttribute("aria-selected", "true");
    expect(previewTab).toHaveAttribute("aria-selected", "false");
    expect(
      (await fileBrowser.findAllByTestId("code-line")).length,
    ).toBeGreaterThan(0);

    await user.click(previewTab);

    expect(
      await fileBrowser.findByRole("heading", { name: "Getting started" }),
    ).toBeTruthy();
    expect(fileBrowser.getByText("Treq").tagName).toBe("STRONG");
    expect(previewTab).toHaveAttribute("aria-selected", "true");
  });

  it("shows humanized modified time and reveals absolute timestamp on hover in file header", async () => {
    const { workspacePath } = await setupWorkspace(
      "feat/filebrowser-edit-time-test",
      {
        "main.ts": "const x = 1;\n",
      },
    );
    const knownMtime = new Date(Date.now() - 2 * 60 * 1000);
    const mainTsPath = path.join(workspacePath, "main.ts");
    fs.utimesSync(mainTsPath, knownMtime, knownMtime);

    await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-edit-time-test",
      "main.ts",
    );

    await screen.findByRole("button", { name: /back/i });
    const relativeEditTime = await screen.findByText(/Modified .* ago/i);
    expect(relativeEditTime).toBeTruthy();

    await user.hover(relativeEditTime);

    const timestampMatches = await screen.findAllByText(
      new RegExp(`\\b${knownMtime.getFullYear().toString()}\\b`),
    );
    expect(timestampMatches.length).toBeGreaterThan(0);
  });

  it("supports in-file search open, navigation, highlighting, and close", async () => {
    await setupWorkspace("feat/filebrowser-search-test", {
      "search-target.ts":
        "function alpha() {\n  return 'alpha';\n}\n\nfunction beta() {\n  return 'beta';\n}\n",
    });

    await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-search-test",
      "search-target.ts",
    );

    await waitFor(() => {
      expect(screen.getAllByTestId("code-line").length).toBeGreaterThan(0);
    });
    await user.keyboard("{Control>}f{/Control}");

    const searchInput = await screen.findByPlaceholderText("Find");
    await user.type(searchInput, "function");

    await screen.findByText("1 of 2");
    await waitFor(() => {
      expect(document.querySelectorAll("mark").length).toBeGreaterThan(0);
    });

    await user.keyboard("{Enter}");
    await screen.findByText("2 of 2");

    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await screen.findByText("1 of 2");

    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByPlaceholderText("Find")).toBeNull();
    });
    await waitFor(() => {
      expect(document.querySelectorAll("mark")).toHaveLength(0);
    });
  });

  it("shows file tree context menu actions for files and directories", async () => {
    const { workspacePath } = await setupWorkspace(
      "feat/filebrowser-context-menu-test",
      {
        "menu-target.ts": "export const menuTarget = true;\n",
        "src/nested.ts": "export const nested = true;\n",
      },
    );

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-context-menu-test",
      "menu-target.ts",
    );

    const [fileEntry] = await fileBrowser.findAllByText("menu-target.ts");
    fireEvent.contextMenu(fileEntry);
    await screen.findByText("Copy relative path");
    await user.click(await screen.findByTestId("copy-relative-path"));
    expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(
      "menu-target.ts",
    );

    fireEvent.contextMenu(fileEntry);
    await user.click(await screen.findByTestId("copy-full-path"));
    expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(
      `${workspacePath}/menu-target.ts`,
    );

    fireEvent.contextMenu(fileEntry);
    const openInMenu = await screen.findByText("Open in...");
    fireEvent.pointerMove(openInMenu);
    fireEvent.mouseEnter(openInMenu);
    fireEvent.keyDown(openInMenu, { key: "ArrowRight" });
    await screen.findByText("Open in Finder");
    await user.click(await screen.findByText("Open in Finder"));
    await waitFor(() => {
      expect(revealItemInDir).toHaveBeenLastCalledWith(
        `${workspacePath}/menu-target.ts`,
      );
    });

    const directoryEntry = await fileBrowser.findByText("src");
    fireEvent.contextMenu(directoryEntry);
    await screen.findByText("Copy relative path");
    await screen.findByText("Copy full path");
    await screen.findByText("Open in...");
  });

  it("wraps a long line within its own row without overlapping neighbors", async () => {
    const longLine =
      "export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(({className, ...props}, ref) => <input ref={ref} className={cn('flex h-9 w-full rounded-md border border-zinc-200 bg-white px-3 py-1 text-sm outline-none focus:ring-2 focus:ring-zinc-300', className)} {...props} />);";
    await setupWorkspace("feat/filebrowser-long-line-wrap-test", {
      "input.tsx": [
        "import * as React from 'react';",
        "import {cn} from '../../lib/utils';",
        longLine,
        "Input.displayName = 'Input';",
        "",
      ].join("\n"),
    });

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-long-line-wrap-test",
      "input.tsx",
    );

    const lines = await fileBrowser.findAllByTestId("code-line");
    const longRow = lines.find((line) =>
      (line.textContent ?? "").includes("forwardRef"),
    );
    expect(longRow).toBeTruthy();
    expect(longRow).toHaveClass("overflow-hidden");

    const content = within(longRow as HTMLElement).getByTestId(
      "code-line-content",
    );
    expect(content).toHaveClass("whitespace-pre-wrap");
    expect(content).not.toHaveClass("overflow-x-auto");
    expect((longRow as HTMLElement).style.height).toBe("");
    expect((longRow as HTMLElement).style.minHeight).toBe("18px");

    const neighbor = lines.find((line) =>
      (line.textContent ?? "").includes("displayName"),
    );
    expect(neighbor?.textContent).not.toContain("forwardRef");
  });

  it("keeps selected text when pointer moves to another code line before copy", async () => {
    await setupWorkspace("feat/filebrowser-selection-persist-test", {
      "selection-target.ts":
        "const selectedSnippet = 'persist_me';\nconst otherLine = 'cursor moved';\n",
    });

    await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-selection-persist-test",
      "selection-target.ts",
    );

    await waitFor(() => {
      expect(screen.getAllByTestId("code-line").length).toBeGreaterThan(1);
    });

    const [firstLine, secondLine] = screen.getAllByTestId("code-line");
    const firstLineContent = within(firstLine).getByTestId("code-line-content");

    const range = document.createRange();
    range.selectNodeContents(firstLineContent);

    const selection = window.getSelection();
    expect(selection).not.toBeNull();
    selection!.removeAllRanges();
    selection!.addRange(range);
    const selectedText = selection!.toString();
    expect(selectedText).toContain("selectedSnippet");

    fireEvent.mouseEnter(secondLine);
    fireEvent.mouseMove(secondLine);

    await user.copy();

    expect(await navigator.clipboard.readText()).toBe(selectedText);
  });

  it("copies the selected file path and shows copied icon feedback", async () => {
    await setupWorkspace("feat/filebrowser-copy-path-test", {
      "copy-me.ts": "export const value = 1;\n",
    });

    const fileBrowser = await openWorkspaceCodeBrowser(
      user,
      "feat/filebrowser-copy-path-test",
      "copy-me.ts",
    );

    const copyButton = await fileBrowser.findByRole("button", {
      name: "Copy file path",
    });
    await user.click(copyButton);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("copy-me.ts");
    expect(
      await fileBrowser.findByRole("button", { name: "File path copied" }),
    ).toBeTruthy();
  });
});
