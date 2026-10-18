// @include-parallel
import * as React from "react";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { openUrl } from "@tauri-apps/plugin-opener";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiChecklistTool } from "../../src/lib/gpui-screen";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { render, screen } from "../test-utils";

const FAKE_TOOL = path.resolve(__dirname, "../fake-agent/fake-agent.sh");

function tool(
  snapshot: Awaited<ReturnType<typeof findGpuiScreen<"onboarding">>>,
  binary: string,
): GpuiChecklistTool | undefined {
  return snapshot.checklist.groups
    .flatMap((g) => g.tools)
    .find((t) => t.binary === binary);
}

describe("GPUI onboarding screen", () => {
  let searchDir: string;

  beforeEach(() => {
    searchDir = fs.mkdtempSync(path.join(os.tmpdir(), "treq-gpui-onb-"));
    const which = (process.env.PATH ?? "")
      .split(path.delimiter)
      .map((dir) => path.join(dir, "which"))
      .find((candidate) => fs.existsSync(candidate));
    if (!which) throw new Error("`which` must be on PATH");
    fs.symlinkSync(which, path.join(searchDir, "which"));
    process.env.TREQ_TEST_PREREQ_PATH = searchDir;
    window.history.replaceState({}, "", "/");
  });

  afterEach(() => {
    delete process.env.TREQ_TEST_PREREQ_PATH;
    fs.rmSync(searchDir, { recursive: true, force: true });
  });

  it("draws the same checklist React draws", async () => {
    fs.symlinkSync(FAKE_TOOL, path.join(searchDir, "claude"));
    render(<Dashboard />);

    const snapshot = await findGpuiScreen("onboarding", (s) => {
      expect(tool(s, "claude")?.installed).toBe(true);
    });
    expect(snapshot.checklist.groups.map((g) => g.label)).toEqual([
      "Required",
      "Agent CLI (install at least one)",
      "Optional",
    ]);
    expect(tool(snapshot, "git")).toMatchObject({
      installed: false,
      required: true,
    });
    expect(tool(snapshot, "gh")?.note).toBe(
      "Needed for pull requests and issues",
    );
    // An agent is installed, so the agent group is not flagged.
    expect(snapshot.checklist.groups[1].error).toBe(false);
    expect(snapshot.remoteLabel).toBeTruthy();
  });

  it("flags the agent group when no agent CLI is installed", async () => {
    render(<Dashboard />);
    const snapshot = await findGpuiScreen("onboarding", (s) => {
      expect(tool(s, "git")?.installed).toBe(false);
    });
    expect(snapshot.checklist.groups[1].error).toBe(true);
  });

  it("opens install links and re-checks through React's handlers", async () => {
    render(<Dashboard />);
    await findGpuiScreen("onboarding", (s) => {
      expect(tool(s, "git")?.installed).toBe(false);
    });

    sendGpuiScreenAction({
      kind: "openUrl",
      url: "https://git-scm.com/downloads",
    });
    expect(vi.mocked(openUrl)).toHaveBeenCalledWith(
      "https://git-scm.com/downloads",
    );

    fs.symlinkSync(FAKE_TOOL, path.join(searchDir, "git"));
    sendGpuiScreenAction({ kind: "recheckPrerequisites" });
    await findGpuiScreen("onboarding", (s) => {
      expect(tool(s, "git")?.installed).toBe(true);
    });
  });

  it("brings the webview back while a React dialog is open", async () => {
    render(<Dashboard />);
    await findGpuiScreen("onboarding");

    sendGpuiScreenAction({ kind: "openRemote" });
    await screen.findByRole("dialog");
    await vi.waitFor(() => expect(gpuiScreen()).toBeNull());
  });
});
