import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../test-utils";
import userEvent from "@testing-library/user-event";
import { ChangesDiffViewer } from "../../src/components/changes-diff-viewer/ChangesDiffViewerMain";
import * as api from "../../src/lib/api";

// Mock the API
vi.mock("../../src/lib/api", async () => {
  const actual = await vi.importActual("../../src/lib/api");
  return {
    ...actual,
    jjGetChangedFiles: vi.fn(),
    jjGetFileHunks: vi.fn(),
    getDiffCache: vi.fn(),
    clearPendingReview: vi.fn(),
  };
});

describe("Cancel/Discard review feature", () => {
  const renderComponent = () =>
    render(
      <ChangesDiffViewer
        workspacePath="/test/workspace"
        repoPath="/test/repo"
        workspaceId={1}
        initialSelectedFile={null}
      />,
    );

  const setupReviewMode = async () => {
    // Wait for file to load
    await waitFor(() => {
      expect(screen.getAllByText(/test\.txt/).length).toBeGreaterThan(0);
    });

    // Expand the file
    const fileElements = screen.getAllByText(/test\.txt/);
    await userEvent.click(fileElements[0]);

    // Wait for add comment button
    await waitFor(() => {
      const buttons = screen.queryAllByRole("button", { name: /add comment/i });
      expect(buttons.length).toBeGreaterThan(0);
    });

    // Click add comment
    const addCommentButtons = screen.getAllByRole("button", {
      name: /add comment/i,
    });
    await userEvent.click(addCommentButtons[0]);

    // Type comment
    const textarea = screen.getByPlaceholderText(/add a comment/i);
    await userEvent.type(textarea, "Test comment");

    // Submit comment
    const submitButtons = screen.getAllByRole("button", {
      name: /add comment/i,
    });
    const submitButton = submitButtons.find(
      (btn) => btn.textContent === "Add Comment",
    );
    if (submitButton) await userEvent.click(submitButton);

    // Wait for comment to appear (will be in multiple places: inline + review panel)
    await waitFor(() => {
      expect(screen.getAllByText("Test comment").length).toBeGreaterThan(0);
    });
  };

  const clickDiscardButton = async () => {
    // Find the Discard button specifically in the alert dialog (not the header bar button)
    // The dialog button will be inside the AlertDialogContent
    const discardButtons = await screen.findAllByRole("button", {
      name: /discard/i,
    });
    // The dialog action button should be the last one (header bar button is first)
    const dialogDiscardButton = discardButtons[discardButtons.length - 1];
    if (dialogDiscardButton) await userEvent.click(dialogDiscardButton);
  };

  beforeEach(() => {
    vi.clearAllMocks();

    vi.mocked(api.jjGetChangedFiles).mockResolvedValue([
      {
        path: "test.txt",
        status: "M",
        previous_path: null,
      },
    ]);

    vi.mocked(api.jjGetFileHunks).mockResolvedValue([
      {
        id: "hunk-1",
        header: "@@ -1,1 +1,1 @@",
        lines: ["-old line", "+new line"],
        patch: "...",
      },
    ]);

    vi.mocked(api.getDiffCache).mockResolvedValue([]);
    vi.mocked(api.clearPendingReview).mockResolvedValue();
  });

  it("should show cancel button when in review mode", async () => {
    renderComponent();
    await setupReviewMode();

    // Should show discard button (cancel button renamed to discard in normal mode)
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /^discard$/i }),
      ).toBeInTheDocument();
    });
  });

  it("should show confirmation dialog on cancel click", async () => {
    renderComponent();
    await setupReviewMode();

    // Click discard button
    const discardButton = screen.getByRole("button", { name: /^discard$/i });
    await userEvent.click(discardButton);

    // Should show confirmation dialog
    await waitFor(() => {
      expect(screen.getByText(/discard review\?/i)).toBeInTheDocument();
      expect(screen.getByText(/this will discard/i)).toBeInTheDocument();
    });
  });

  it("should clear all comments when confirmed", async () => {
    renderComponent();
    await setupReviewMode();

    // Click discard button
    const discardButton = await screen.findByRole("button", {
      name: /^discard$/i,
    });
    await userEvent.click(discardButton);

    // Click discard in dialog
    await clickDiscardButton();

    // Comment should be gone
    await waitFor(() => {
      expect(screen.queryByText("Test comment")).not.toBeInTheDocument();
    });
  });

  it("should not clear anything when dialog is dismissed", async () => {
    renderComponent();
    await setupReviewMode();

    // Click discard button
    const discardButton = await screen.findByRole("button", {
      name: /^discard$/i,
    });
    await userEvent.click(discardButton);

    // Click "Keep reviewing" button
    const keepButton = await screen.findByRole("button", {
      name: /keep reviewing/i,
    });
    await userEvent.click(keepButton);

    // Comment should still exist
    await waitFor(() => {
      expect(screen.getAllByText("Test comment").length).toBeGreaterThan(0);
    });
  });

  it("should close dialog when 'Keep Reviewing' is clicked", async () => {
    renderComponent();
    await setupReviewMode();

    // Click discard button
    const discardButton = await screen.findByRole("button", {
      name: /^discard$/i,
    });
    await userEvent.click(discardButton);

    // Verify dialog is shown
    await waitFor(() => {
      expect(screen.getByText(/discard review\?/i)).toBeInTheDocument();
    });

    // Click "Keep reviewing" button
    const keepButton = await screen.findByRole("button", {
      name: /keep reviewing/i,
    });
    await userEvent.click(keepButton);

    // Verify dialog is closed
    await waitFor(() => {
      expect(screen.queryByText(/discard review\?/i)).not.toBeInTheDocument();
    });

    // Verify comment is still present
    expect(screen.getAllByText("Test comment").length).toBeGreaterThan(0);
  });

  it("should clear persisted review from database on confirm", async () => {
    renderComponent();
    await setupReviewMode();

    // Clear mock calls
    vi.mocked(api.clearPendingReview).mockClear();

    // Click discard and confirm
    const discardButton = await screen.findByRole("button", {
      name: /^discard$/i,
    });
    await userEvent.click(discardButton);

    await clickDiscardButton();

    // Verify clearPendingReview was called
    await waitFor(() => {
      expect(api.clearPendingReview).toHaveBeenCalledWith("/test/repo", 1);
    });
  });

  describe("Cancel button text based on mode", () => {
    it("should show 'Discard' button text in normal review mode (no conflicts)", async () => {
      renderComponent();
      await setupReviewMode();

      // Should show "Discard" button in normal review mode
      await waitFor(() => {
        const discardButton = screen.getByRole("button", {
          name: /^discard$/i,
        });
        expect(discardButton).toBeInTheDocument();
        expect(discardButton).toHaveTextContent("Discard");
      });
    });

    it("should show 'Reset' button text in conflict resolution mode", async () => {
      // Mock a file with conflicts
      vi.mocked(api.jjGetChangedFiles).mockResolvedValue([
        {
          path: "conflict.txt",
          status: "M",
          previous_path: null,
        },
      ]);

      vi.mocked(api.jjGetFileHunks).mockResolvedValue([
        {
          id: "hunk-1",
          header: "@@ -1,7 +1,7 @@",
          lines: [
            "+<<<<<<< Conflict 1 of 1",
            "+%%%%%%%",
            "+-old line",
            "++++++++",
            "+new line",
            "+>>>>>>> Conflict 1 of 1 ends",
          ],
          patch: "...",
        },
      ]);

      render(
        <ChangesDiffViewer
          workspacePath="/test/workspace"
          repoPath="/test/repo"
          workspaceId={1}
          initialSelectedFile={null}
          conflictedFiles={["conflict.txt"]}
        />,
      );

      // Wait for conflict to be displayed
      await waitFor(() => {
        expect(screen.getAllByText(/Conflict 1 of 1/).length).toBeGreaterThan(
          0,
        );
      });

      // Click add comment to enter review mode
      const addCommentButtons = screen.getAllByRole("button", {
        name: /add comment/i,
      });
      const conflictButton = addCommentButtons.find(
        (btn) => btn.textContent === "Add comment",
      );
      await userEvent.click(conflictButton!);

      // Type comment
      const textarea = screen.getByPlaceholderText(/add a comment/i);
      await userEvent.type(textarea, "Test comment");

      // Submit comment
      const submitButtons = screen.getAllByRole("button", {
        name: /add comment/i,
      });
      const submitButton = submitButtons.find(
        (btn) => btn.textContent === "Add Comment",
      );
      await userEvent.click(submitButton!);

      // Should show "Reset" button in conflict mode
      await waitFor(() => {
        const resetButton = screen.getByRole("button", { name: /^reset$/i });
        expect(resetButton).toBeInTheDocument();
        expect(resetButton).toHaveTextContent("Reset");
      });
    });
  });
});
