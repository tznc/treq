// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, openRepo } from "../utils";
import { render, screen } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";

import { waitFor as waitForSnapshot } from "@testing-library/react";
import { gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";

async function openGpuiSettings() {
  await waitForSnapshot(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

describe("Settings integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("opens settings page, switches between tabs, and saves application settings", async () => {
    render(<Dashboard />);

    await openGpuiSettings();

    await screen.findByText("Settings");
    await screen.findByRole("button", { name: /save settings/i });

    const repositoryTab = await screen.findByRole("tab", {
      name: /repository/i,
    });
    expect(repositoryTab).toHaveAttribute("data-state", "active");

    const terminalPane = await screen.findByTestId("workspace-terminal-pane");
    expect(terminalPane).not.toBeVisible();

    const applicationTab = await screen.findByRole("tab", {
      name: /application/i,
    });
    expect(screen.getByRole("tab", { name: /account/i })).toBeVisible();
    expect(screen.getByRole("tab", { name: /integrations/i })).toBeVisible();
    expect(screen.getByRole("tab", { name: /skills/i })).toBeVisible();
    await user.click(applicationTab);
    await screen.findByLabelText(/theme/i);
    const fontSizeSelect = await screen.findByLabelText(/font size/i);

    expect(
      screen.queryByLabelText(/branch name pattern/i),
    ).not.toBeInTheDocument();

    await user.selectOptions(fontSizeSelect, "Large");
    await user.click(screen.getByRole("button", { name: /save settings/i }));
    await screen.findByText("Settings Saved");
    await screen.findByText(/application settings updated successfully/i);

    await user.click(repositoryTab);
    await screen.findByLabelText(/branch name pattern/i);
    await screen.findByLabelText(/claude code model/i);
    expect(
      screen.queryByLabelText(/symlinked directories/i),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/theme/i)).not.toBeInTheDocument();
  });

  it("returns to the previous page when settings is closed", async () => {
    render(<Dashboard />);

    await openGpuiSettings();
    expect(
      await screen.findByRole("heading", { name: "Settings" }),
    ).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Close" }));

    expect(
      screen.queryByRole("heading", { name: "Settings" }),
    ).not.toBeInTheDocument();
    expect(await screen.findByTestId("workspace-terminal-pane")).toBeVisible();
  });
});
