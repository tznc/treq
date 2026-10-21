// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, openRepo, writeRepoFile } from "../utils";
import { render, screen, waitFor } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";

import { waitFor as waitForSnapshot } from "@testing-library/react";
import { gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";

async function openGpuiSettings() {
  await waitForSnapshot(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

describe("Repository YAML config sync", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("shows a sync card and disables the settings .treq/config.yaml provides, with their real values", async () => {
    const { repoPath } = createTestRepo(false);
    await writeRepoFile(
      repoPath,
      ".treq/config.yaml",
      ["default_model: opus", "default_agent: claude"].join("\n"),
    );
    openRepo(repoPath);

    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));

    await screen.findByText(/synced from \.treq\/config\.yaml/i);

    const modelSelect = await screen.findByLabelText(/claude code model/i);
    expect(modelSelect).toBeDisabled();
    expect(modelSelect).toHaveValue("opus");

    const agentSelect = screen.getByLabelText(/default agent/i);
    expect(agentSelect).toBeDisabled();
    expect(agentSelect).toHaveValue("claude");

    const branchPatternInput = screen.getByLabelText(/branch name pattern/i);
    expect(branchPatternInput).not.toBeDisabled();
    expect(branchPatternInput).toHaveValue("treq/{name}");
  });

  it("shows no card and leaves every input enabled when no .treq/config.yaml exists", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));

    expect(
      await screen.findByLabelText(/branch name pattern/i),
    ).not.toBeDisabled();
    expect(screen.getByLabelText(/claude code model/i)).not.toBeDisabled();
    expect(screen.getByLabelText(/default agent/i)).not.toBeDisabled();
    expect(screen.getByLabelText(/included files/i)).not.toBeDisabled();
    expect(
      screen.queryByText(/synced from \.treq\/config\.yaml/i),
    ).not.toBeInTheDocument();
  });

  it("disables Included Files/Directories and shows the config's joined value", async () => {
    const { repoPath } = createTestRepo(false);
    await writeRepoFile(
      repoPath,
      ".treq/config.yaml",
      "included_copy_files:\n  - .env\n  - .env.local\n",
    );
    openRepo(repoPath);

    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));

    const includedFilesTextarea =
      await screen.findByLabelText(/included files/i);
    expect(includedFilesTextarea).toBeDisabled();
    expect(includedFilesTextarea).toHaveValue(".env\n.env.local");
  });

  it("syncs a newly written .treq/config.yaml when Settings is reopened", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));
    expect(
      await screen.findByLabelText(/branch name pattern/i),
    ).not.toBeDisabled();
    await user.click(screen.getByRole("button", { name: /^close$/i }));

    await writeRepoFile(
      repoPath,
      ".treq/config.yaml",
      'branch_name_pattern: "release/{name}"\n',
    );

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));

    const branchPatternInput =
      await screen.findByLabelText(/branch name pattern/i);
    expect(branchPatternInput).toBeDisabled();
    expect(branchPatternInput).toHaveValue("release/{name}");
  });

  it("updates disabled field values when Reload is clicked after editing the file", async () => {
    const { repoPath } = createTestRepo(false);
    await writeRepoFile(repoPath, ".treq/config.yaml", "default_model: opus\n");
    openRepo(repoPath);

    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));

    await screen.findByText(/synced from \.treq\/config\.yaml/i);
    expect(await screen.findByLabelText(/claude code model/i)).toHaveValue(
      "opus",
    );

    await writeRepoFile(
      repoPath,
      ".treq/config.yaml",
      "default_model: sonnet\n",
    );
    await user.click(await screen.findByRole("button", { name: /reload/i }));

    await waitFor(() => {
      expect(screen.getByLabelText(/claude code model/i)).toHaveValue("sonnet");
    });
  });

  it("flips which fields are disabled when Reload changes which fields the file sets", async () => {
    const { repoPath } = createTestRepo(false);
    await writeRepoFile(
      repoPath,
      ".treq/config.yaml",
      ["default_model: opus", "default_agent: claude"].join("\n"),
    );
    openRepo(repoPath);

    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /repository/i }));

    expect(await screen.findByLabelText(/claude code model/i)).toBeDisabled();
    expect(screen.getByLabelText(/branch name pattern/i)).not.toBeDisabled();

    await writeRepoFile(
      repoPath,
      ".treq/config.yaml",
      'branch_name_pattern: "release/{name}"\n',
    );
    await user.click(await screen.findByRole("button", { name: /reload/i }));

    await waitFor(() => {
      const branchPatternInput = screen.getByLabelText(/branch name pattern/i);
      expect(branchPatternInput).toBeDisabled();
      expect(branchPatternInput).toHaveValue("release/{name}");
      expect(screen.getByLabelText(/claude code model/i)).not.toBeDisabled();
      expect(screen.getByLabelText(/default agent/i)).not.toBeDisabled();
    });
  });
});
