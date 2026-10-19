import { useLocation } from "wouter";
import {
  type GitHubStateFilter,
  githubDetailPath,
  githubListPath,
  githubNewItemPath,
  githubPrReviewPath,
  githubTabPath,
} from "../lib/githubRoutes";
import { isGpuiShellHost } from "../lib/gpui-shell";
import { type GpuiScreenAction, useGpuiScreen } from "../lib/gpui-screen";
import { requestOpenWorkspaceLogs } from "../lib/open-logs-request";
import type { GitHubIssueAttachment } from "../lib/promptAttachments";
import {
  type ChecksTarget,
  rerunWorkspaceSetup,
  runWorkspaceJob,
  runWorkspaceWorkflow,
  useRunningCheckKeys,
} from "../hooks/useWorkspaceChecks";

/** GPUI draws the GitHub view's Issues and Pull Requests tabs. */
export function gpuiDrawsGithub(): boolean {
  return (
    isGpuiShellHost() &&
    window.__TREQ_WEB_UI__ !== true &&
    window.__TREQ_REACT_GITHUB__ !== true
  );
}

interface ChecksRun extends ChecksTarget {
  filename: string;
  jobId: string | null;
}

/**
 * Draws GitHubPanel's Issues and Pull Requests tabs in GPUI
 * (src-tauri/src/gpui_shell/screen/github.rs), which reads and changes
 * issues and pull requests through the backend itself. React keeps the URL
 * route it shows, the merge queue tab and the PR review page (which it
 * draws when GPUI navigates there), and the workflow runs started from this
 * window.
 */
export function useGpuiGithubScreen(params: {
  /** null: React draws the panel. */
  tab: "issues" | "prs" | null;
  repoPath: string;
  filter: GitHubStateFilter;
  selected: number | null;
  creating: boolean;
  /** The merge queue tab, when the build offers it. */
  mergeQueue: { pro: boolean } | null;
  onStartPromptFromIssue?: (issue: GitHubIssueAttachment) => void;
  onOpenWorkspace?: (workspaceId: number) => void;
}) {
  const [, navigate] = useLocation();
  const runningChecks = useRunningCheckKeys();
  const { tab, filter } = params;

  const view = tab
    ? ({
        kind: "github",
        repoPath: params.repoPath,
        tab,
        filter,
        selected: params.selected,
        creating: params.creating,
        mergeQueue: params.mergeQueue,
        runningChecks,
      } as const)
    : null;

  const press = (id: string) => {
    if (!tab) return;
    const [name, arg = ""] = id.split(":");
    switch (name) {
      case "github.tab":
        navigate(
          arg === "merge-queue"
            ? githubTabPath("merge-queue")
            : githubListPath(arg as "issues" | "prs"),
        );
        break;
      case "github.filter":
        navigate(githubListPath(tab, arg as GitHubStateFilter));
        break;
      case "github.select":
        navigate(githubDetailPath(tab, Number(arg), filter));
        break;
      case "github.new":
        navigate(githubNewItemPath(tab, filter));
        break;
      case "github.close":
        navigate(githubListPath(tab, filter), { replace: true });
        break;
      case "github.created":
        navigate(githubDetailPath(tab, Number(arg), filter), {
          replace: true,
        });
        break;
      case "github.review":
        navigate(githubPrReviewPath(Number(arg), filter));
        break;
      case "github.openWorkspace":
        params.onOpenWorkspace?.(Number(arg));
        break;
      case "github.openLogs":
        requestOpenWorkspaceLogs(Number(arg));
        params.onOpenWorkspace?.(Number(arg));
        break;
      default:
        break;
    }
  };

  useGpuiScreen(view, (action: GpuiScreenAction) => {
    if (action.kind === "press") {
      press(action.id);
      return;
    }
    if (action.kind !== "field") return;
    const value = action.value as Record<string, unknown>;
    if (action.field === "github.startPrompt") {
      params.onStartPromptFromIssue?.(
        value as unknown as GitHubIssueAttachment,
      );
    } else if (action.field === "github.checks.run") {
      const run = value as unknown as ChecksRun;
      const done = run.jobId
        ? runWorkspaceJob(run, run.filename, run.jobId)
        : runWorkspaceWorkflow(run, run.filename);
      void done.catch(() => undefined);
    } else if (action.field === "github.checks.setup") {
      void rerunWorkspaceSetup(value as unknown as ChecksTarget).catch(
        () => undefined,
      );
    }
  });
}
