// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../../src/components/GitHubPanel";
import { useOpenLogsRequest } from "../../src/lib/open-logs-request";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { render, screen } from "../test-utils";
import { createTestRepo } from "../utils";

const api = vi.hoisted(() => ({
  runWorkflow: vi.fn(),
  ghListIssues: vi.fn(),
  ghListPrs: vi.fn(),
}));

vi.mock("../../src/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/lib/api")>()),
  runWorkflow: api.runWorkflow,
  ghListIssues: api.ghListIssues,
  ghListPrs: api.ghListPrs,
}));

const press = (id: string) => sendGpuiScreenAction({ kind: "press", id });

describe("GPUI GitHub screen", () => {
  let repoPath: string;

  beforeEach(() => {
    window.__TREQ_REACT_GITHUB__ = false;
    window.location.hash = "#/github/issues/open";
    ({ repoPath } = createTestRepo(false));
    api.runWorkflow.mockReset();
    api.ghListIssues.mockReset();
    api.ghListPrs.mockReset();
  });

  afterEach(() => {
    window.location.hash = "";
  });

  it("sends the route, and React fetches no lists", async () => {
    render(<GitHubPanel repoPath={repoPath} gpui />);

    const github = await findGpuiScreen("github");
    expect(Object.keys(github).sort()).toEqual([
      "creating",
      "dark",
      "filter",
      "kind",
      "mergeQueue",
      "repoPath",
      "runningChecks",
      "selected",
      "tab",
    ]);
    expect(github).toMatchObject({
      repoPath,
      tab: "issues",
      filter: "open",
      selected: null,
      creating: false,
      runningChecks: [],
    });
    expect(api.ghListIssues).not.toHaveBeenCalled();
    expect(api.ghListPrs).not.toHaveBeenCalled();
    expect(screen.queryByText("Issues")).not.toBeInTheDocument();
  });

  it("moves through the URL as GPUI presses", async () => {
    render(<GitHubPanel repoPath={repoPath} gpui />);
    await findGpuiScreen("github");

    press("github.tab:prs");
    await findGpuiScreen("github", (s) => expect(s.tab).toBe("prs"));
    expect(window.location.hash).toBe("#/github/prs/open");

    press("github.filter:draft");
    await findGpuiScreen("github", (s) => expect(s.filter).toBe("draft"));

    press("github.select:12");
    await findGpuiScreen("github", (s) => expect(s.selected).toBe(12));
    expect(window.location.hash).toBe("#/github/prs/draft/12");

    press("github.close");
    await findGpuiScreen("github", (s) => expect(s.selected).toBeNull());

    press("github.new");
    await findGpuiScreen("github", (s) => expect(s.creating).toBe(true));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    press("github.created:31");
    await findGpuiScreen("github", (s) => {
      expect(s.creating).toBe(false);
      expect(s.selected).toBe(31);
    });

    press("github.review:31");
    await waitFor(() =>
      expect(window.location.hash).toBe("#/github/prs/draft/31/review"),
    );
  });

  it("hands issues, workspaces and logs to its callbacks", async () => {
    const onStartPromptFromIssue = vi.fn();
    const onOpenWorkspace = vi.fn();
    render(
      <GitHubPanel
        repoPath={repoPath}
        gpui
        onStartPromptFromIssue={onStartPromptFromIssue}
        onOpenWorkspace={onOpenWorkspace}
      />,
    );
    await findGpuiScreen("github");

    const issue = { number: 4, url: "https://x/4", title: "Crash" };
    sendGpuiScreenAction({
      kind: "field",
      field: "github.startPrompt",
      value: issue,
    });
    expect(onStartPromptFromIssue).toHaveBeenCalledWith(issue);

    press("github.openWorkspace:7");
    expect(onOpenWorkspace).toHaveBeenLastCalledWith(7);

    press("github.openLogs:8");
    expect(useOpenLogsRequest.getState().workspaceId).toBe(8);
    expect(onOpenWorkspace).toHaveBeenLastCalledWith(8);
  });

  it("runs workflows for GPUI and reports them running", async () => {
    let finish: (value: never[]) => void = () => undefined;
    api.runWorkflow.mockImplementation(
      () => new Promise((resolve) => (finish = resolve)),
    );
    render(<GitHubPanel repoPath={repoPath} gpui />);
    await findGpuiScreen("github");

    const target = {
      repoPath,
      workspaceId: 3,
      workspacePath: `${repoPath}/.treq/workspaces/feat`,
    };
    sendGpuiScreenAction({
      kind: "field",
      field: "github.checks.run",
      value: { ...target, filename: "ci.yml", jobId: null },
    });
    expect(api.runWorkflow).toHaveBeenCalledWith(
      repoPath,
      "ci.yml",
      3,
      target.workspacePath,
    );
    await findGpuiScreen("github", (s) =>
      expect(s.runningChecks).toEqual(["3:ci.yml"]),
    );

    finish([]);
    await findGpuiScreen("github", (s) => expect(s.runningChecks).toEqual([]));
  });

  it("leaves remote repositories to React", async () => {
    const pushes = () =>
      vi
        .mocked(invoke)
        .mock.calls.filter(([cmd]) => cmd === "gpui_screen_update").length;
    const before = pushes();
    render(<GitHubPanel repoPath={repoPath} />);
    expect(await screen.findByText("Pull Requests")).toBeInTheDocument();
    expect(pushes()).toBe(before);
    expect(gpuiScreen() ?? null).toBeNull();
  });
});
