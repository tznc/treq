// @include-serial
import * as React from "react";
import fs from "node:fs";
import path from "node:path";
import { fireEvent } from "@testing-library/react";
import * as api from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import {
  type Workspace,
  createWorkspace,
  getWorkspaces,
  listCommits,
} from "../../../src/lib/api";
import { render, screen, waitFor } from "../../test-utils";
import userEvent from "@testing-library/user-event";
import {
  addSingleReviewComment as addReviewComment,
  clickChangedFile,
  openReviewTab,
  waitForChangedFile,
} from "../review/comments-helpers";

const REVIEW_FILE = "reviews-flow.txt";

async function setupWorkspaceWithChange(branchName: string): Promise<{
  repoPath: string;
  workspace: Workspace;
  fileName: string;
}> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (item) => item.id === workspaceId,
  );
  if (!workspace) {
    throw new Error(`Workspace not found for id ${workspaceId}`);
  }

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    REVIEW_FILE,
    "first review line\nsecond review line\n",
  );

  return { fileName: REVIEW_FILE, repoPath, workspace };
}

async function addSingleReviewComment(
  user: ReturnType<typeof userEvent.setup>,
  fileName: string,
  comment: string,
) {
  await clickChangedFile(fileName);
  await addReviewComment(user, comment);
}

async function openReviewWithChange(branchName: string) {
  const fixture = await setupWorkspaceWithChange(branchName);
  return fixture;
}

async function expectCommitCreated(
  repoPath: string,
  workspaceId: number,
  commitMessage: string,
) {
  await waitFor(async () => {
    const log = await listCommits(repoPath, workspaceId, false, undefined, 30);
    expect(
      log.commits.some((commit) => commit.description === commitMessage),
    ).toBe(true);
  });
}

describe("ShowWorkspace - Reviews integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("is able to create a commit from Review tab", async () => {
    const branchName = "feat/reviews-commit";
    const commitMessage = "Add integration reviews test commit";
    const { repoPath, workspace } = await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);
    await waitForChangedFile(REVIEW_FILE);

    await user.type(
      await screen.findByPlaceholderText("Message"),
      commitMessage,
    );
    await user.click(await screen.findByRole("button", { name: /^commit$/i }));
    await expectCommitCreated(repoPath, workspace.id, commitMessage);
  });

  it("is able to review a change and start an agent", async () => {
    const branchName = "feat/reviews-agent";
    const { repoPath, workspace } = await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);
    const createSessionSpy = vi.spyOn(api, "createSession");

    await addSingleReviewComment(
      user,
      REVIEW_FILE,
      "Please tighten this change",
    );
    await user.click(
      await screen.findByRole("button", { name: /finish review/i }),
    );
    await user.click(await screen.findByRole("button", { name: /^plan$/i }));

    await waitFor(() => {
      expect(createSessionSpy).toHaveBeenCalledWith(
        repoPath,
        workspace.id,
        "Code Review",
      );
    });
  });

  it("is able to mark a file as viewed and back to unviewed", async () => {
    const branchName = "feat/reviews-viewed";
    await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);

    const viewedCheckbox = await screen.findByRole("checkbox", {
      name: "Viewed",
    });
    expect(viewedCheckbox).toHaveAttribute("aria-checked", "false");

    await user.click(viewedCheckbox);
    await waitFor(() => {
      expect(viewedCheckbox).toHaveAttribute("aria-checked", "true");
      expect(
        screen.getByRole("button", { name: "Expand file diff" }),
      ).toBeInTheDocument();
    });

    await user.click(viewedCheckbox);
    await waitFor(() => {
      expect(viewedCheckbox).toHaveAttribute("aria-checked", "false");
      expect(
        screen.getByRole("button", { name: "Collapse file diff" }),
      ).toBeInTheDocument();
    });
  }, 15_000);

  it("is able to expand and collapse a file diff", async () => {
    const branchName = "feat/reviews-expand-collapse";
    await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);
    await waitForChangedFile(REVIEW_FILE);

    await user.click(
      screen.getByRole("button", { name: "Collapse file diff" }),
    );
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Expand file diff" }),
      ).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "Expand file diff" }));
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Collapse file diff" }),
      ).toBeInTheDocument();
    });
  });
});

async function setupStagingWorkspaceWithChange(branchName: string): Promise<{
  repoPath: string;
  workspace: Workspace;
}> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    "stage-test.txt",
    "line one\nline two\nline three\n",
  );

  return { repoPath, workspace };
}

describe("ShowWorkspace - File staging integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("stages a file and shows it in the Selected section", async () => {
    const branchName = "feat/staging-test";
    await setupStagingWorkspaceWithChange(branchName);

    render(<Dashboard />);
    await user.click(await findSidebarBranchElement(branchName));

    const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
    await user.click(reviewTab);
    await screen.findByRole("tab", { name: /^Changes/, selected: true });

    await waitFor(() =>
      expect(screen.getAllByText("stage-test.txt").length).toBeGreaterThan(0),
    );

    const fileRows = screen.getAllByText("stage-test.txt");
    await user.click(fileRows[0]);

    const stageButton = await screen.findByTitle("Stage file(s) for commit");
    await user.click(stageButton);

    await screen.findByText("Selected");
  });

  it("collapses and expands file diffs with toggle button", async () => {
    const branchName = "feat/collapse-test";
    await setupStagingWorkspaceWithChange(branchName);

    render(<Dashboard />);
    await user.click(await findSidebarBranchElement(branchName));

    const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
    await user.click(reviewTab);
    await screen.findByRole("tab", { name: /^Changes/, selected: true });

    await waitFor(() =>
      expect(screen.getAllByText("stage-test.txt").length).toBeGreaterThan(0),
    );

    const collapseBtn = await screen.findByRole("button", {
      name: "Collapse file diff",
    });
    await user.click(collapseBtn);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Expand file diff" }),
      ).toBeInTheDocument();
    });
  });
});

async function createTwoWorkspacesWithDirtySource() {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const sourceId = await api.createWorkspace(repoPath, "feat/source");
  const destId = await api.createWorkspace(repoPath, "feat/dest");
  const workspaces = await api.getWorkspaces(repoPath);
  const source = workspaces.find((w) => w.id === sourceId)!;
  const dest = workspaces.find((w) => w.id === destId)!;
  expect(source).toBeTruthy();
  expect(dest).toBeTruthy();

  const sourcePath = resolveWorkspacePath(repoPath, source.workspace_path);
  const destPath = resolveWorkspacePath(repoPath, dest.workspace_path);
  const fileName = "move-me.txt";
  writeWorkspaceFile(sourcePath, fileName, "content to move\n");

  return { repoPath, source, dest, sourcePath, destPath, fileName };
}

async function openReviewForBranch(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(branchName));
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
}

async function waitForChangedFileText(fileName: string) {
  await screen.findAllByText(fileName);
}

function createDataTransfer(): DataTransfer {
  const store = new Map<string, string>();
  const types: string[] = [];
  return {
    dropEffect: "move",
    effectAllowed: "all",
    files: [] as unknown as FileList,
    items: [] as unknown as DataTransferItemList,
    types,
    clearData: (format?: string) => {
      if (format) {
        store.delete(format);
        const idx = types.indexOf(format);
        if (idx >= 0) types.splice(idx, 1);
      } else {
        store.clear();
        types.length = 0;
      }
    },
    getData: (format: string) => store.get(format) ?? "",
    setData: (format: string, data: string) => {
      store.set(format, data);
      if (!types.includes(format)) types.push(format);
    },
    setDragImage: () => {},
  } as DataTransfer;
}

function dragAndDrop(sourceEl: HTMLElement, dropTarget: HTMLElement) {
  const dataTransfer = createDataTransfer();
  fireEvent.dragStart(sourceEl, { dataTransfer });
  fireEvent.dragEnter(dropTarget, { dataTransfer });
  fireEvent.dragOver(dropTarget, { dataTransfer });
  fireEvent.drop(dropTarget, { dataTransfer });
  fireEvent.dragEnd(sourceEl, { dataTransfer });
}

describe("Review tab - drag changes to workspace sidebar", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("asks for confirmation then moves the file to the dropped workspace", async () => {
    const { source, dest, sourcePath, destPath, fileName } =
      await createTwoWorkspacesWithDirtySource();
    await openReviewForBranch(user, source.branch_name);

    await waitForChangedFileText(fileName);

    const [sidebarFile] = screen.getAllByText(fileName);
    const dragRow = sidebarFile.closest("[draggable='true']") as HTMLElement;
    expect(dragRow).toBeTruthy();
    const destRow = await findSidebarBranchElement(dest.branch_name);

    dragAndDrop(dragRow, destRow);

    await screen.findByText(/Move 1 file to feat\/dest\?/);
    await user.click(await screen.findByRole("button", { name: /^Move$/ }));

    await waitFor(() => {
      expect(fs.existsSync(path.join(sourcePath, fileName))).toBe(false);
      expect(fs.existsSync(path.join(destPath, fileName))).toBe(true);
    });
    await waitFor(() => {
      expect(screen.queryAllByText(fileName)).toHaveLength(0);
    });

    expect(fs.existsSync(path.join(sourcePath, fileName))).toBe(false);
    expect(fs.existsSync(path.join(destPath, fileName))).toBe(true);
    expect(fs.readFileSync(path.join(destPath, fileName), "utf8")).toContain(
      "content to move",
    );
  });

  it("keeps changes when the confirmation is cancelled", async () => {
    const { source, dest, sourcePath, fileName } =
      await createTwoWorkspacesWithDirtySource();
    await openReviewForBranch(user, source.branch_name);

    await waitForChangedFileText(fileName);

    const [sidebarFile] = screen.getAllByText(fileName);
    const dragRow = sidebarFile.closest("[draggable='true']") as HTMLElement;
    const destRow = await findSidebarBranchElement(dest.branch_name);

    dragAndDrop(dragRow, destRow);

    await screen.findByText(/Move 1 file to feat\/dest\?/);
    await user.click(await screen.findByRole("button", { name: /Cancel/i }));

    await waitFor(() => {
      expect(screen.queryByText(/Move 1 file to feat\/dest\?/)).toBeNull();
    });

    expect(fs.existsSync(path.join(sourcePath, fileName))).toBe(true);
    expect(screen.getAllByText(fileName).length).toBeGreaterThan(0);
  });

  it("moves the full selection when dragging one selected file", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const sourceId = await api.createWorkspace(repoPath, "feat/multi-src");
    const destId = await api.createWorkspace(repoPath, "feat/multi-dest");
    const workspaces = await api.getWorkspaces(repoPath);
    const source = workspaces.find((w) => w.id === sourceId)!;
    const dest = workspaces.find((w) => w.id === destId)!;
    const sourcePath = resolveWorkspacePath(repoPath, source.workspace_path);
    const destPath = resolveWorkspacePath(repoPath, dest.workspace_path);

    writeWorkspaceFile(sourcePath, "a.txt", "aaa\n");
    writeWorkspaceFile(sourcePath, "b.txt", "bbb\n");

    await openReviewForBranch(user, source.branch_name);

    await waitForChangedFileText("a.txt");
    await waitForChangedFileText("b.txt");

    await user.click(screen.getByTitle(/Select all/i));

    const [bFile] = screen.getAllByText("b.txt");
    const dragRow = bFile.closest("[draggable='true']") as HTMLElement;
    const destRow = await findSidebarBranchElement(dest.branch_name);

    dragAndDrop(dragRow, destRow);

    await screen.findByText(/Move 2 files to feat\/multi-dest\?/);
    await user.click(await screen.findByRole("button", { name: /^Move$/ }));

    await waitFor(() => {
      expect(fs.existsSync(path.join(sourcePath, "a.txt"))).toBe(false);
      expect(fs.existsSync(path.join(sourcePath, "b.txt"))).toBe(false);
      expect(fs.existsSync(path.join(destPath, "a.txt"))).toBe(true);
      expect(fs.existsSync(path.join(destPath, "b.txt"))).toBe(true);
    });
    await waitFor(() => {
      expect(screen.queryAllByText("a.txt")).toHaveLength(0);
      expect(screen.queryAllByText("b.txt")).toHaveLength(0);
    });

    expect(fs.existsSync(path.join(sourcePath, "a.txt"))).toBe(false);
    expect(fs.existsSync(path.join(sourcePath, "b.txt"))).toBe(false);
    expect(fs.existsSync(path.join(destPath, "a.txt"))).toBe(true);
    expect(fs.existsSync(path.join(destPath, "b.txt"))).toBe(true);
  });
});

async function createDirtyWorkspace(branchName: string, fileName: string) {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await api.createWorkspace(repoPath, branchName);
  const workspace = (await api.getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  expect(workspace).toBeTruthy();

  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace!.workspace_path,
  );
  writeWorkspaceFile(workspacePath, fileName, "dirty working copy content\n");

  return { repoPath, workspace: workspace!, workspacePath };
}

describe("Review tab - discard changes", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("discards all uncommitted changes from the Changes tab", async () => {
    const fileName = "discard-all.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-all",
      fileName,
    );
    await openReviewTab(user, "feat/discard-all");
    await waitForChangedFile(fileName);

    await user.click(
      await screen.findByRole("button", { name: /discard all changes/i }),
    );
    await screen.findByText("Discard all changes?");
    const confirmButtons = screen.getAllByRole("button", {
      name: /discard all changes/i,
    });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    await waitFor(() => {
      expect(screen.queryByTestId(`file-row-${fileName}`)).toBeNull();
    });

    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(false);
  });

  it("discards a selected file from the Review sidebar", async () => {
    const fileName = "discard-one.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-one",
      fileName,
    );
    await openReviewTab(user, "feat/discard-one");
    await waitForChangedFile(fileName);

    const [sidebarFile] = screen.getAllByText(fileName);
    await user.click(sidebarFile);

    await user.click(await screen.findByTitle("Discard selected files"));

    await waitFor(() => {
      expect(screen.queryByTestId(`file-row-${fileName}`)).toBeNull();
    });

    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(false);
  });

  it("keeps changes when canceling the discard-all confirmation", async () => {
    const fileName = "discard-cancel.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-cancel",
      fileName,
    );
    await openReviewTab(user, "feat/discard-cancel");
    await waitForChangedFile(fileName);

    await user.click(
      await screen.findByRole("button", { name: /discard all changes/i }),
    );
    await screen.findByText("Discard all changes?");
    await screen.clickByText("Cancel");

    expect(screen.getAllByText(fileName).length).toBeGreaterThan(0);
    expect(screen.queryByText("Discard all changes?")).toBeNull();
    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(true);
  });

  it("undoes discard-all and restores the previous working-copy changes", async () => {
    const fileName = "discard-undo.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-undo",
      fileName,
    );
    await openReviewTab(user, "feat/discard-undo");
    await waitForChangedFile(fileName);

    await user.click(
      await screen.findByRole("button", { name: /discard all changes/i }),
    );
    await screen.findByText("Discard all changes?");
    const confirmButtons = screen.getAllByRole("button", {
      name: /discard all changes/i,
    });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    await waitFor(() => {
      expect(screen.queryByTestId(`file-row-${fileName}`)).toBeNull();
    });
    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(false);

    await user.click(await screen.findByRole("button", { name: "Undo" }));

    await waitFor(() => {
      expect(screen.getByTestId(`file-row-${fileName}`)).toBeTruthy();
    });
    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(true);
  });
});
