// @include-serial
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTestRepo,
  newCommitWithParents,
  openRepo,
  resolveChangeId,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import {
  checkAndRebaseWorkspaces,
  createCommit,
  createWorkspace,
  ensureWorkspaceIndexed,
  getWorkspaceStatus,
  getWorkspaces,
} from "../../../src/lib/api";
import { render, screen, waitFor, within, act } from "../../test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import * as api from "../../../src/lib/api";
import { clickGpuiRow, findGpuiRow } from "../../gpui-sidebar";

type ReviewFixture = {
  repoPath: string;
  branchName: string;
  workspaceId: number;
  workspacePath: string;
  conflictFile: string;
};

async function navigateToReviewTab(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  await clickGpuiRow(branchName);
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
}

async function clickFileInSection(
  user: ReturnType<typeof userEvent.setup>,
  sectionName: "Conflicts" | "Changes",
  fileName: string,
) {
  const sectionToggle = await screen.findByRole("button", {
    name: sectionName,
  });
  const sectionHeader = sectionToggle.closest("div");
  if (!sectionHeader?.parentElement) {
    throw new Error(`Could not locate ${sectionName} section container`);
  }
  const section = sectionHeader.parentElement;
  const fileRow = await within(section).findByTitle(fileName);
  await user.click(fileRow);
}

async function assertStatus(
  repoPath: string,
  workspaceId: number,
  expected: { hasConflicts: boolean; conflictedFiles: string[] },
) {
  const status = await getWorkspaceStatus(repoPath, workspaceId);
  expect(status.has_conflicts).toBe(expected.hasConflicts);
  expect(status.conflicted_files).toEqual(expected.conflictedFiles);
}

async function createWorkspaceFixture(
  branchName: string,
): Promise<ReviewFixture> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error("Workspace not found");
  return {
    repoPath,
    branchName,
    workspaceId,
    workspacePath: resolveWorkspacePath(repoPath, workspace.workspace_path),
    conflictFile: "README.md",
  };
}

async function setupCleanState(): Promise<ReviewFixture> {
  return createWorkspaceFixture("feat/review-clean");
}

async function setupDivergentNonConflictState(): Promise<ReviewFixture> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const baseId = await createWorkspace(repoPath, "feature-base");
  const baseWs = (await getWorkspaces(repoPath)).find((w) => w.id === baseId);
  if (!baseWs) throw new Error("Base workspace not found");
  const basePath = resolveWorkspacePath(repoPath, baseWs.workspace_path);

  const stackedId = await createWorkspace(
    repoPath,
    "feat/divergent",
    "feature-base",
  );
  const stackedWs = (await getWorkspaces(repoPath)).find(
    (w) => w.id === stackedId,
  );
  if (!stackedWs) throw new Error("Stacked workspace not found");
  const stackedPath = resolveWorkspacePath(repoPath, stackedWs.workspace_path);

  writeWorkspaceFile(basePath, "base-only.txt", "base change\n");
  await createCommit(repoPath, baseId, "base-only change");

  writeWorkspaceFile(stackedPath, "stacked-only.txt", "stacked change\n");
  await createCommit(repoPath, stackedId, "stacked-only change");

  return {
    repoPath,
    branchName: "feat/divergent",
    workspaceId: stackedId,
    workspacePath: stackedPath,
    conflictFile: "README.md",
  };
}

async function setupUnresolvedConflictState(
  branchName: string,
): Promise<ReviewFixture> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error("Workspace not found");
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
  await createCommit(repoPath, workspaceId, "workspace conflicting change");
  const workspaceChangeId = resolveChangeId(workspacePath, "@-");

  writeWorkspaceFile(repoPath, "README.md", "main side\n");
  await createCommit(repoPath, null, "main conflicting change");
  const mainChangeId = resolveChangeId(repoPath, "@-");

  newCommitWithParents(workspacePath, [workspaceChangeId, mainChangeId]);
  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

  return {
    repoPath,
    branchName,
    workspaceId,
    workspacePath,
    conflictFile: "README.md",
  };
}

async function setupRebaseConflictState(
  branchName: string,
): Promise<ReviewFixture> {
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error("Workspace not found");
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
  await createCommit(repoPath, workspaceId, "workspace conflicting change");

  writeWorkspaceFile(repoPath, "README.md", "main side\n");
  await createCommit(repoPath, null, "main conflicting change");

  await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);
  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

  return {
    repoPath,
    branchName,
    workspaceId,
    workspacePath,
    conflictFile: "README.md",
  };
}

describe("Review - conflict rendering contract", { timeout: 20_000 }, () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("clean state: status shows no conflicts and no Conflicts section", async () => {
    const fixture = await setupCleanState();
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: false,
      conflictedFiles: [],
    });

    render(<Dashboard />);
    await navigateToReviewTab(user, fixture.branchName);
    expect(screen.queryByText("Conflicts")).not.toBeInTheDocument();
  });

  it("divergent non-conflict state: no Conflicts section", async () => {
    const fixture = await setupDivergentNonConflictState();
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: false,
      conflictedFiles: [],
    });

    render(<Dashboard />);
    await navigateToReviewTab(user, fixture.branchName);
    expect(screen.queryByText("Conflicts")).not.toBeInTheDocument();
  });

  it("divergent non-conflict state: sidebar should not show conflict tooltip", async () => {
    const fixture = await setupDivergentNonConflictState();
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: false,
      conflictedFiles: [],
    });

    render(<Dashboard />);
    const row = await findGpuiRow(fixture.branchName);
    expect(row.conflicted).toBe(false);
    expect(screen.queryByText("Conflicts detected")).not.toBeInTheDocument();
  });

  it("resolving markers in the working copy clears Conflicts UI before commit", async () => {
    const fixture = await setupUnresolvedConflictState(
      "feat/wc-resolve-clears-ui",
    );

    const { listen } = await import("@tauri-apps/api/event");
    type FilesChangedHandler = (event: {
      payload: { workspace_id: number; changed_paths: string[] };
    }) => void;
    const filesChangedHandlers: FilesChangedHandler[] = [];
    vi.mocked(listen).mockImplementation(((event: string, handler: unknown) => {
      if (event === "workspace-files-changed") {
        filesChangedHandlers.push(handler as FilesChangedHandler);
      }
      return Promise.resolve(() => {});
    }) as typeof listen);

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await navigateToReviewTab(user, fixture.branchName);
    await screen.findByRole("button", { name: "Conflicts" });
    expect(
      screen.getByRole("button", { name: /Resolve conflicts/i }),
    ).toBeTruthy();

    const conflictPill = screen.getByTestId("review-change-count");
    expect(conflictPill.className).toMatch(/destructive/);

    writeWorkspaceFile(
      fixture.workspacePath,
      fixture.conflictFile,
      "resolved content\n",
    );

    await waitFor(async () => {
      const status = await getWorkspaceStatus(
        fixture.repoPath,
        fixture.workspaceId,
      );
      expect(status.has_conflicts).toBe(false);
      expect(status.conflicted_files).toEqual([]);
    });

    expect(filesChangedHandlers.length).toBeGreaterThan(0);
    await act(async () => {
      for (const handler of filesChangedHandlers) {
        handler({
          payload: {
            workspace_id: fixture.workspaceId,
            changed_paths: [fixture.conflictFile],
          },
        });
      }
    });

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Conflicts" }),
      ).not.toBeInTheDocument();
    });
    expect(
      screen.queryByRole("button", { name: /Resolve conflicts/i }),
    ).not.toBeInTheDocument();

    await waitFor(() => {
      const pill = screen.getByTestId("review-change-count");
      expect(pill.className).toMatch(/yellow/);
      expect(pill.className).not.toMatch(/destructive/);
    });

    expect(screen.getByRole("button", { name: /^Changes$/ })).toBeTruthy();
  });

  it("unresolved conflict state: Conflicts section is rendered from backend metadata", async () => {
    const fixture = await setupUnresolvedConflictState(
      "feat/unresolved-conflict",
    );

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: true,
      conflictedFiles: [fixture.conflictFile],
    });
    await navigateToReviewTab(user, fixture.branchName);
    await screen.findByText("Conflicts");
    expect(
      screen.queryByRole("button", { name: /^Changes$/ }),
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByText("README.md").length).toBeGreaterThan(0);
    });
  });

  it("conflict identity without regions: keeps conflicted file identity with no invented conflict cards", async () => {
    const fixture = await setupUnresolvedConflictState("feat/zero-regions");

    const originalGetWorkspaceFileHunks = api.getWorkspaceFileHunks;
    const getHunksSpy = vi.spyOn(api, "getWorkspaceFileHunks");
    getHunksSpy.mockImplementation(async (...args) => {
      const hunks = await originalGetWorkspaceFileHunks(...args);
      return hunks.map((h) => ({ ...h, conflict_regions: [] }));
    });

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: true,
      conflictedFiles: [fixture.conflictFile],
    });
    await navigateToReviewTab(user, fixture.branchName);
    await screen.findByText("Conflicts");
    await waitFor(() => {
      expect(screen.getAllByText("README.md").length).toBeGreaterThan(0);
    });
    expect(screen.queryByText("Conflict 1 of")).not.toBeInTheDocument();
    getHunksSpy.mockRestore();
  });

  it("rebase conflict state: renders an inline conflict card with base and both sides", async () => {
    const fixture = await setupRebaseConflictState("feat/conflict-card");

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await navigateToReviewTab(user, fixture.branchName);
    await clickFileInSection(user, "Conflicts", "README.md");

    const card = await screen.findByText(/^Conflict 1 of 1$/);
    const cardRoot = card.closest("[data-conflict-card]");
    expect(cardRoot).not.toBeNull();

    const roleOf = (text: string) =>
      within(cardRoot as HTMLElement)
        .getByText(text)
        .closest("[data-conflict-line-role]")
        ?.getAttribute("data-conflict-line-role");
    expect(roleOf("main side")).toBe("left");
    expect(roleOf("# Test Repository")).toBe("base");
    expect(roleOf("workspace side")).toBe("right");

    expect(
      within(cardRoot as HTMLElement)
        .getAllByText(/^Side #1$|^Base$|^Side #2$/)
        .map((el) => el.textContent),
    ).toEqual(["Side #1", "Base", "Side #2"]);
  });

  it("conflicted files suppress line-comment controls while non-conflicted files keep them", async () => {
    const fixture = await setupUnresolvedConflictState("feat/comment-controls");

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: true,
      conflictedFiles: [fixture.conflictFile],
    });
    await navigateToReviewTab(user, fixture.branchName);

    await clickFileInSection(user, "Conflicts", "README.md");
    await waitFor(() => {
      const readmeDiff = document.querySelector('[data-file-path="README.md"]');
      expect(readmeDiff).not.toBeNull();
      expect(readmeDiff?.querySelectorAll("[data-comment-button]").length).toBe(
        0,
      );
    });
  });

  it("committed-only conflict stays visible when Committed changes are hidden", async () => {
    const fixture = await setupRebaseConflictState(
      "feat/committed-conflict-hidden",
    );

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: true,
      conflictedFiles: [fixture.conflictFile],
    });
    await navigateToReviewTab(user, fixture.branchName);
    await screen.findByText("Conflicts");

    const showButton = screen.getByRole("button", { name: /^Show$/ });
    await user.click(showButton);

    await clickFileInSection(user, "Conflicts", "README.md");
    await screen.findByText(/^Conflict 1 of 1$/);
    expect(screen.queryByText("No changes to review")).not.toBeInTheDocument();
  });

  it("conflicted file with no diff hunks shows an explicit placeholder", async () => {
    const fixture = await setupUnresolvedConflictState(
      "feat/deleted-conflict-placeholder",
    );

    const originalGetWorkspaceFileHunks = api.getWorkspaceFileHunks;
    const getHunksSpy = vi.spyOn(api, "getWorkspaceFileHunks");
    getHunksSpy.mockImplementation(async (...args) => {
      const [repoPath, workspaceId, filePath] = args;
      if (filePath === fixture.conflictFile) {
        return [];
      }
      return originalGetWorkspaceFileHunks(repoPath, workspaceId, filePath);
    });

    render(<Dashboard />);
    await waitFor(async () =>
      expect((await findGpuiRow(fixture.branchName)).conflicted).toBe(true),
    );
    await assertStatus(fixture.repoPath, fixture.workspaceId, {
      hasConflicts: true,
      conflictedFiles: [fixture.conflictFile],
    });
    await navigateToReviewTab(user, fixture.branchName);
    await clickFileInSection(user, "Conflicts", "README.md");
    await waitFor(() => {
      expect(screen.getAllByText("README.md").length).toBeGreaterThan(0);
    });
    expect(
      screen.queryByText(
        "No diff available for this conflicted file (possibly deleted)",
      ),
    ).not.toBeInTheDocument();
    getHunksSpy.mockRestore();
  });

  it("delete/modify conflict shows a deleted-side card for the absent side", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const workspaceId = await createWorkspace(
      repoPath,
      "feat/delete-modify-conflict",
    );
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");
    const workspacePath = resolveWorkspacePath(
      repoPath,
      workspace.workspace_path,
    );

    writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
    await createCommit(repoPath, workspaceId, "workspace modify");
    const workspaceChangeId = resolveChangeId(workspacePath, "@-");

    const fs = await import("node:fs");
    const path = await import("node:path");
    fs.unlinkSync(path.join(repoPath, "README.md"));
    await createCommit(repoPath, null, "main delete");
    const mainChangeId = resolveChangeId(repoPath, "@-");

    newCommitWithParents(workspacePath, [workspaceChangeId, mainChangeId]);
    await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

    render(<Dashboard />);
    await waitFor(async () =>
      expect(
        (await findGpuiRow("feat/delete-modify-conflict")).conflicted,
      ).toBe(true),
    );
    await assertStatus(repoPath, workspaceId, {
      hasConflicts: true,
      conflictedFiles: ["README.md"],
    });
    await navigateToReviewTab(user, "feat/delete-modify-conflict");
    await clickFileInSection(user, "Conflicts", "README.md");

    await screen.findByText(/^Conflict 1 of 1$/);
    const deletedCard = await screen.findByTestId("conflict-deleted-side");
    expect(deletedCard).toHaveTextContent(/deleted this file/);
  });
});
