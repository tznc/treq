import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "../../../test/test-utils";
import userEvent from "@testing-library/user-event";
import { ChangesDiffViewer } from "./ChangesDiffViewerMain";

vi.mock("../../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../../lib/api")>("../../lib/api");
  return {
    ...actual,
    getWorkspaceDiff: vi.fn(),
    getWorkspaceChangedFiles: vi.fn().mockResolvedValue([]),
    getWorkspaceFileHunks: vi.fn().mockResolvedValue([]),
    getDiffCache: vi.fn().mockResolvedValue(null),
    clearPendingReview: vi.fn().mockResolvedValue(undefined),
  };
});

/**
 * Regression: Review-tab committed hunks live in `committedFileHunks`, but
 * multi-line selection / comment-from-selection only consulted `allFileHunks`.
 * Dragging across committed lines therefore never expanded the selection and
 * the + button could not open a multi-line composer.
 */
describe("ChangesDiffViewer committed multi-line comments", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    const api = await import("../../lib/api");
    vi.mocked(api.getWorkspaceDiff).mockResolvedValue({
      uncommitted_files: [],
      committed_files: [
        {
          path: "test.txt",
          status: "M",
          changed_line_count: 3,
          diff_deferred: false,
        },
      ],
      hunks_by_file: [
        {
          path: "test.txt",
          hunks: [
            {
              id: "committed-hunk-1",
              header: "@@ -1,6 +1,7 @@",
              lines: [
                " context line 1",
                "+added line 2",
                "+added line 3",
                "+added line 4",
                " context line 5",
                " context line 6",
              ],
              patch: "...",
            },
          ],
        },
      ],
      too_large_to_render: false,
      render_block_reason: null,
    });
  });

  async function renderCommittedReview() {
    render(
      <ChangesDiffViewer
        workspacePath="/tmp/workspace"
        repoPath="/tmp/repo"
        workspaceId={1}
        initialSelectedFile={null}
        showCommittedChanges={true}
      />,
    );

    await waitFor(() => {
      expect(screen.getAllByText(/test\.txt/).length).toBeGreaterThan(0);
    });

    // Expand the committed file if the hunk lines aren't mounted yet.
    if (document.querySelectorAll("[data-diff-line]").length === 0) {
      const [fileLabel] = screen.getAllByText(/test\.txt/);
      fileLabel.click();
    }

    await waitFor(() => {
      expect(
        document.querySelectorAll("[data-diff-line]").length,
      ).toBeGreaterThan(0);
      expect(screen.getAllByText(/added line 2/).length).toBeGreaterThan(0);
    });
  }

  it("opens a multi-line comment composer on committed hunks", async () => {
    const user = userEvent.setup();
    await renderCommittedReview();

    const [, line2, , line4] = Array.from(
      document.querySelectorAll("[data-diff-line]"),
    );
    const gutter = line2.querySelector("[data-testid='line-gutter']");
    expect(gutter).not.toBeNull();

    fireEvent.mouseDown(gutter!, { button: 0 });
    fireEvent.mouseOver(line4);
    fireEvent.mouseEnter(line4);
    fireEvent.mouseUp(line4);

    const commentBtn = await waitFor(() => {
      const button = line4.querySelector("[data-comment-button]");
      expect(button).not.toBeNull();
      return button as HTMLElement;
    });
    await user.click(commentBtn);

    await screen.findByPlaceholderText(/add a comment/i);
  });
});
