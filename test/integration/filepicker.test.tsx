// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import {
  commitRepoFile,
  createTestRepo,
  gitCommitRepoFile,
  openRepo,
} from "../utils";
import { createWorkspace, ensureWorkspaceIndexed } from "../../src/lib/api";
import {
  render,
  screen,
  settleReactUpdates,
  waitFor,
  within,
} from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import { clickGpuiRow } from "../gpui-sidebar";

describe("FilePicker integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  const openFilePicker = async () => {
    render(<Dashboard />);
    await user.keyboard("{Control>}p{/Control}");
    return screen.findByPlaceholderText("Search files...");
  };

  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    await commitRepoFile(repoPath, "makefile", "all:", "add makefile");
    await commitRepoFile(
      repoPath,
      "src-tauri/.gitignore",
      "target",
      "add gitignore",
    );
    await commitRepoFile(
      repoPath,
      "src/components/Button.tsx",
      "export const Button = () => {};",
      "add Button",
    );
    await ensureWorkspaceIndexed(repoPath, null, repoPath);
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("opens via Ctrl+P and shows indexed files", async () => {
    await openFilePicker();

    await screen.findByText(/Button\.tsx/);
  });

  it("searches files", async () => {
    const input = await openFilePicker();
    await user.type(input, "Button");

    const picker = within(screen.getByTestId("modal"));
    await picker.findByText(/Button\.tsx/);
    await waitFor(() => {
      expect(picker.queryByText(/makefile/)).not.toBeInTheDocument();
    });
  });

  it("selects a result and opens the file", async () => {
    await openFilePicker();

    await user.click(await screen.findByText(/Button\.tsx/));
    expect(
      screen.queryByPlaceholderText("Search files..."),
    ).not.toBeInTheDocument();

    await waitFor(() => {
      expect(
        document.querySelector('[data-testid="code-line-content"]'),
      ).toHaveTextContent("export const Button = () => {};");
    });
  });

  it("shows 'No files found' for a nonexistent query", async () => {
    const input = await openFilePicker();
    await user.type(input, "zzz_nonexistent_file");

    await screen.findByText("No files found");
  });

  it.each([
    ["make file", /makefile/],
    ["taurgit", /src-tauri\/.gitignore/],
  ])("fuzzy searches files with query %s", async (query, expectedFile) => {
    const input = await openFilePicker();
    await user.type(input, query);

    const picker = within(screen.getByTestId("modal"));
    await picker.findByText(expectedFile);
  });
});

describe("FilePicker in a workspace", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    await gitCommitRepoFile(
      repoPath,
      "src/components/Button.tsx",
      "export const Button = () => {};",
      "add Button",
    );
    await gitCommitRepoFile(
      repoPath,
      "src/components/Modal.tsx",
      "export const Modal = () => {};",
      "add Modal",
    );
    await createWorkspace(repoPath, "feat/search");
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("lists workspace files on Cmd+P without a prior file-browser index", async () => {
    render(<Dashboard />);
    await clickGpuiRow("feat/search");
    await screen.findByTestId("show-workspace-header");
    await settleReactUpdates();

    await user.keyboard("{Control>}p{/Control}");

    const input = await screen.findByPlaceholderText("Search files...");
    await screen.findByText(/Button\.tsx/);
    await screen.findByText(/Modal\.tsx/);

    await user.clear(input);
    await user.type(input, "Button");

    await screen.findByText(/Button\.tsx/);
    await waitFor(() => {
      expect(screen.queryByText(/Modal\.tsx/)).not.toBeInTheDocument();
    });
  });
});
