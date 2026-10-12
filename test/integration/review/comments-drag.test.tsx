// @include-serial
import { beforeEach, describe, expect, it } from "vitest";
import { commitWorkspaceFile, createTestRepo, openRepo } from "../../utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import {
  fireEvent,
  screen,
  settleReactUpdates,
  waitFor,
} from "../../test-utils";
import userEvent from "@testing-library/user-event";
import {
  getClickableArea,
  getDiffLines,
  openReviewTab,
  setupWorkspaceWithDiff,
  waitForFileAndLines,
} from "./comments-helpers";

describe("Multi-line selection in diff viewer", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("highlights the dragged lines in either direction and keeps them after mouseup", async () => {
    await setupWorkspaceWithDiff("feat/drag-select");
    await openReviewTab(user, "feat/drag-select");
    await waitForFileAndLines();

    const [, line2, line3, line4] = getDiffLines();

    fireEvent.mouseDown(getClickableArea(line2), { button: 0 });
    fireEvent.mouseOver(line3);
    fireEvent.mouseEnter(line3);
    fireEvent.mouseOver(line4);
    fireEvent.mouseEnter(line4);
    fireEvent.mouseUp(getClickableArea(line4));
    fireEvent.click(getClickableArea(line4));

    await waitFor(() => {
      expect(line2.className).toContain("bg-blue-500/10");
      expect(line3.className).toContain("bg-blue-500/10");
      expect(line4.className).toContain("bg-blue-500/10");
    });

    await settleReactUpdates();

    expect(line2.className).toContain("bg-blue-500/10");
    expect(line3.className).toContain("bg-blue-500/10");
    expect(line4.className).toContain("bg-blue-500/10");

    fireEvent.mouseDown(getClickableArea(line4), { button: 0 });
    fireEvent.mouseOver(line2);
    fireEvent.mouseEnter(line2);
    fireEvent.mouseUp(line2);

    await waitFor(() => {
      expect(line2.className).toContain("bg-blue-500/10");
      expect(line4.className).toContain("bg-blue-500/10");
    });
  });

  it("should open comment input when clicking + after multi-line selection", async () => {
    await setupWorkspaceWithDiff("feat/drag-comment");
    await openReviewTab(user, "feat/drag-comment");
    await waitForFileAndLines();

    const [, line2, , line4] = getDiffLines();

    fireEvent.mouseDown(getClickableArea(line2), { button: 0 });
    fireEvent.mouseOver(line4);
    fireEvent.mouseEnter(line4);
    fireEvent.mouseUp(line4);

    await waitFor(() => {
      expect(line2.className).toContain("bg-blue-500/10");
    });

    const commentBtn = line4.querySelector("[data-comment-button]")!;
    fireEvent.click(commentBtn as HTMLElement);

    await screen.findByPlaceholderText(/add a comment/i);
  });
});

describe("Multi-line selection on committed Review-tab files", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  async function setupCommittedDiff(branchName: string) {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const workspaceId = await createWorkspace(repoPath, branchName);
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");

    await commitWorkspaceFile(
      repoPath,
      { id: workspaceId, path: workspace.workspace_path },
      "test.txt",
      "context line 1\nadded line 2\nadded line 3\nadded line 4\ncontext line 5\ncontext line 6",
      "add test.txt",
    );

    return { repoPath, workspace };
  }

  it("highlights a multi-line drag selection on committed hunks", async () => {
    await setupCommittedDiff("feat/committed-drag");
    await openReviewTab(user, "feat/committed-drag");
    await waitForFileAndLines();

    const [, line2, line3, line4] = getDiffLines();

    fireEvent.mouseDown(getClickableArea(line2), { button: 0 });
    fireEvent.mouseOver(line3);
    fireEvent.mouseEnter(line3);
    fireEvent.mouseOver(line4);
    fireEvent.mouseEnter(line4);
    fireEvent.mouseUp(line4);

    await waitFor(() => {
      expect(line2.className).toContain("bg-blue-500/10");
      expect(line3.className).toContain("bg-blue-500/10");
      expect(line4.className).toContain("bg-blue-500/10");
    });
  });

  it("opens a multi-line comment composer on committed hunks", async () => {
    await setupCommittedDiff("feat/committed-comment");
    await openReviewTab(user, "feat/committed-comment");
    await waitForFileAndLines();

    const [, line2, , line4] = getDiffLines();

    fireEvent.mouseDown(getClickableArea(line2), { button: 0 });
    fireEvent.mouseOver(line4);
    fireEvent.mouseEnter(line4);
    fireEvent.mouseUp(line4);

    await waitFor(() => {
      expect(line2.className).toContain("bg-blue-500/10");
      expect(line4.className).toContain("bg-blue-500/10");
    });

    const commentBtn = line4.querySelector("[data-comment-button]")!;
    fireEvent.click(commentBtn as HTMLElement);

    await screen.findByPlaceholderText(/add a comment/i);
  });
});
