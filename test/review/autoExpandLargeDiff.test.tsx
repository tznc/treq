import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../test-utils";
import userEvent from "@testing-library/user-event";
import * as api from "../../src/lib/api";
import { ChangesDiffViewer } from "../../src/components/changes-diff-viewer/ChangesDiffViewerMain";

vi.mock("../../src/lib/api", async () => {
  const actual = await vi.importActual("../../src/lib/api");
  return {
    ...actual,
    jjGetChangedFiles: vi.fn(),
    jjGetFileHunks: vi.fn(),
    getDiffCache: vi.fn(),
    loadPendingReview: vi.fn(),
    savePendingReview: vi.fn(),
    clearPendingReview: vi.fn(),
    setDiffCache: vi.fn(),
  };
});

describe("Auto-expand large diffs when clicking file in file list", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getDiffCache).mockResolvedValue([]);
    vi.mocked(api.savePendingReview).mockResolvedValue();
    vi.mocked(api.clearPendingReview).mockResolvedValue();
    vi.mocked(api.loadPendingReview).mockResolvedValue([]);
    vi.mocked(api.setDiffCache).mockResolvedValue();
  });

  it("should auto-expand large diff (>250 lines) when clicking file in file list", async () => {
    vi.mocked(api.jjGetChangedFiles).mockResolvedValue([
      { path: "large-file.txt", status: "M", previous_path: null },
    ]);

    const largeDiffLines = Array.from(
      { length: 300 },
      (_, i) => `+added line ${i + 1}`,
    );

    vi.mocked(api.jjGetFileHunks).mockResolvedValue([
      {
        id: "hunk-1",
        header: "@@ -1,0 +1,300 @@",
        lines: largeDiffLines,
        patch: "...",
      },
    ]);

    render(
      <ChangesDiffViewer
        workspacePath="/test/workspace"
        repoPath="/test/repo"
        workspaceId={1}
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      expect(screen.getAllByText(/large-file\.txt/).length).toBeGreaterThan(0);
    });

    const fileElements = screen.getAllByText(/large-file\.txt/);
    await userEvent.click(fileElements[0]);

    await screen.findByText("added line 50");
    await screen.findByText("added line 288");

    expect(screen.queryByText(/Large diff/i)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /view changes/i }),
    ).not.toBeInTheDocument();
  });
});
