// @include-parallel
import * as React from "react";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { render, screen, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

import { waitFor as waitForSnapshot } from "@testing-library/react";
import { gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";

async function openGpuiSettings() {
  await waitForSnapshot(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

const FAKE_TOOL = path.resolve(__dirname, "../fake-agent/fake-agent.sh");

describe("onboarding setup checklist", () => {
  let searchDir: string;
  let user: ReturnType<typeof userEvent.setup>;

  const installTool = (name: string) =>
    fs.symlinkSync(FAKE_TOOL, path.join(searchDir, name));

  const findRow = async (label: string) =>
    within(
      await screen.findByRole("region", { name: "Setup checklist" }),
    ).findByRole("listitem", { name: label });

  const linkSystemWhichForTheLookup = () => {
    const which = (process.env.PATH ?? "")
      .split(path.delimiter)
      .map((dir) => path.join(dir, "which"))
      .find((candidate) => fs.existsSync(candidate));
    if (!which) throw new Error("`which` must be on PATH");
    fs.symlinkSync(which, path.join(searchDir, "which"));
  };

  beforeEach(() => {
    searchDir = fs.mkdtempSync(path.join(os.tmpdir(), "treq-prereq-path-"));
    linkSystemWhichForTheLookup();
    process.env.TREQ_TEST_PREREQ_PATH = searchDir;
    window.history.replaceState({}, "", "/");
    user = userEvent.setup();
  });

  afterEach(() => {
    delete process.env.TREQ_TEST_PREREQ_PATH;
    fs.rmSync(searchDir, { recursive: true, force: true });
  });

  it("marks installed tools and links each missing one to its install page", async () => {
    installTool("claude");
    render(<Dashboard />);

    expect(
      await within(await findRow("Claude Code")).findByText("Installed"),
    ).toBeInTheDocument();
    expect(
      await within(await findRow("GitHub CLI")).findByText(
        "Needed for pull requests and issues",
      ),
    ).toBeInTheDocument();

    await user.click(
      await within(await findRow("Git")).findByRole("button", {
        name: "Install Git",
      }),
    );
    expect(vi.mocked(openUrl)).toHaveBeenCalledWith(
      "https://git-scm.com/downloads",
    );
    expect(
      await within(await findRow("GitHub CLI")).findByRole("button", {
        name: "Install GitHub CLI",
      }),
    ).toBeInTheDocument();
  });

  it("picks up a tool installed after the first check", async () => {
    render(<Dashboard />);
    await within(await findRow("Git")).findByRole("button", {
      name: "Install Git",
    });

    installTool("git");
    await user.click(
      await screen.findByRole("button", { name: "Check again" }),
    );

    expect(
      await within(await findRow("Git")).findByText("Installed"),
    ).toBeInTheDocument();
  });

  it("lists the same tools in Application settings", async () => {
    installTool("git");
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    render(<Dashboard />);

    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /application/i }));

    expect(
      await within(await findRow("Git")).findByText("Installed"),
    ).toBeInTheDocument();
    expect(
      await within(await findRow("Codex")).findByRole("button", {
        name: "Install Codex",
      }),
    ).toBeInTheDocument();
  });
});
