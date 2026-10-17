// @include-parallel
import { findGpuiToast } from "../gpui-sidebar";
import * as React from "react";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { open } from "@tauri-apps/plugin-dialog";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { getSetting, setSetting } from "../../src/lib/api";
import { render, screen } from "../test-utils";

describe("opening a folder that is not a Git repository", () => {
  let folder: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    folder = fs.mkdtempSync(path.join(os.tmpdir(), "treq-not-git-"));
    window.history.replaceState({}, "", "/");
    user = userEvent.setup();
  });

  afterEach(() => {
    fs.rmSync(folder, { recursive: true, force: true });
  });

  it("shows an error and leaves the folder and last-opened repo untouched", async () => {
    await setSetting("last_opened_repo_path", "/previous/repo");
    vi.mocked(open).mockResolvedValueOnce(folder);
    render(<Dashboard />);

    await user.click(
      await screen.findByRole("button", { name: "Open Repository" }),
    );

    const toast = await findGpuiToast("Failed to open repository");
    expect(toast.description).toBe(
      `${folder} is not a Git repository. Open a folder that contains a .git directory.`,
    );
    expect(
      await screen.findByRole("button", { name: "Open Repository" }),
    ).toBeInTheDocument();
    expect(fs.existsSync(path.join(folder, ".treq"))).toBe(false);
    expect(await getSetting("last_opened_repo_path")).toBe("/previous/repo");
  });
});
