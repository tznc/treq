// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  writeRepoFile,
} from "../utils";
import { render, screen, waitFor } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace, trustRepo } from "../../src/lib/api";
import { clickGpuiRow } from "../gpui-sidebar";

describe("CommandPalette integration", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;

  beforeEach(() => {
    const values = new Map<string, string>();
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
        removeItem: (key: string) => values.delete(key),
        clear: () => values.clear(),
      },
    });
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("opens on Ctrl+K and shows Search Files command", async () => {
    render(<Dashboard />);

    await user.keyboard("{Control>}k{/Control}");

    await screen.findByText("Search Files");
  });

  it("shows description for Search Files command", async () => {
    render(<Dashboard />);

    await user.keyboard("{Control>}k{/Control}");

    await screen.findByText("Jump to a file in the repository");
  });

  it("always shows Go to Home and Go to Settings commands", async () => {
    render(<Dashboard />);

    await user.keyboard("{Control>}k{/Control}");

    await screen.findByText("Go to Home");
    await screen.findByText("Go to Settings");
  });

  it("clicking Search Files opens the file picker and closes the palette", async () => {
    render(<Dashboard />);

    await user.keyboard("{Control>}k{/Control}");
    await user.click(await screen.findByText("Search Files"));

    await screen.findByPlaceholderText("Search files...");

    await waitFor(() => {
      expect(
        screen.queryByPlaceholderText("Type a command or search..."),
      ).not.toBeInTheDocument();
    });
  });

  it("opens the same stacked workspace dialog as the Stack button", async () => {
    await createWorkspace(repoPath, "feat/parent");
    render(<Dashboard />);
    await clickGpuiRow("feat/parent");

    await user.keyboard("{Control>}k{/Control}");
    await user.click(await screen.findByText("Create Workspace"));

    expect(await screen.findByText("Stack a new Workspace")).toBeVisible();
    expect(
      screen.getByText(
        "Create a new workspace stacked on feat/parent. Optionally move commits or file changes.",
      ),
    ).toBeVisible();
    await waitFor(() => {
      expect(
        screen.queryByPlaceholderText("Type a command or search..."),
      ).not.toBeInTheDocument();
    });
  });

  it("runs the workspace's checks from Run checks…", async () => {
    await writeRepoFile(
      repoPath,
      ".treq/workflows/ci.yaml",
      "name: CI\njobs:\n  test:\n    steps:\n      - name: Echo\n        run: echo ok\n",
    );
    await trustRepo(repoPath);
    await createWorkspace(repoPath, "feat/palette-checks");
    render(<Dashboard />);
    await clickGpuiRow("feat/palette-checks");

    await user.keyboard("{Control>}k{/Control}");
    expect(await screen.findByText("Run check: CI")).toBeTruthy();
    await user.click(await screen.findByText("Run checks…"));

    expect(
      await screen.findByText("Checks passed", {}, { timeout: 20_000 }),
    ).toBeTruthy();
  });
});
