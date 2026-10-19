// @include-parallel
import * as React from "react";
import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../../src/components/GitHubPanel";
import { PrReviewScreen } from "../../src/components/github-panel/PrReviewPage";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { findGpuiToast } from "../gpui-sidebar";
import { render, screen } from "../test-utils";
import { createTestRepo } from "../utils";

const dequeue = vi.hoisted(() => vi.fn());

vi.mock("../../src/lib/features", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../src/lib/features")>();
  return { ...actual, FEATURES: { ...actual.FEATURES, mergeQueue: true } };
});
vi.mock("../../src/hooks/useMergeQueueStatus", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../src/hooks/useMergeQueueStatus")
  >()),
  useDequeueBranches: () => ({ mutate: dequeue, isPending: false }),
}));

const press = (id: string) => sendGpuiScreenAction({ kind: "press", id });
const field = (name: string, value: unknown[] | number | boolean) =>
  sendGpuiScreenAction({ kind: "field", field: name, value });

describe("GPUI PR review screen", () => {
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
  });

  it("asks GPUI for the PR and leaves through React", async () => {
    const onClose = vi.fn();
    render(
      <PrReviewScreen
        repoPath={repoPath}
        prNumber={42}
        onClose={onClose}
        gpui
      />,
    );

    const review = await findGpuiScreen("prReview");
    expect(review).toEqual({
      kind: "prReview",
      dark: false,
      repoPath,
      prNumber: 42,
    });
    press("prReview.close");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("starts the agent review GPUI asks for", async () => {
    const onStartAgentReview = vi
      .fn()
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error("no agent"));
    render(
      <PrReviewScreen
        repoPath={repoPath}
        prNumber={7}
        onClose={vi.fn()}
        onStartAgentReview={onStartAgentReview}
        gpui
      />,
    );
    await findGpuiScreen("prReview");

    const start = (agent: string | null) =>
      sendGpuiScreenAction({
        kind: "field",
        field: "prReview.startAgentReview",
        value: { prompt: "Review it", agent },
      });
    start("codex");
    expect(onStartAgentReview).toHaveBeenLastCalledWith("Review it", "codex");
    start(null);
    expect(onStartAgentReview).toHaveBeenLastCalledWith("Review it", undefined);
    expect((await findGpuiToast("Failed to start review")).description).toBe(
      "no agent",
    );
  });

  it("leaves GPUI to the webview without the flag", async () => {
    render(
      <PrReviewScreen repoPath={repoPath} prNumber={1} onClose={vi.fn()} />,
    );
    await screen.findByTestId("pr-review-page");
    expect(gpuiScreen()?.kind).not.toBe("prReview");
  });
});

describe("GPUI merge queue tab", () => {
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    window.location.hash = "#/github/merge-queue";
    dequeue.mockReset();
  });

  it("sends the panel's queue state to GPUI", async () => {
    render(<GitHubPanel repoPath={repoPath} gpui />);

    const queue = await findGpuiScreen("mergeQueue", (s) => {
      expect(s.remoteLoading).toBe(false);
    });
    expect(queue).toMatchObject({
      kind: "mergeQueue",
      repoPath,
      fullName: null,
      isPro: false,
      entries: [],
      dequeuing: false,
      showMergedHistory: false,
      historyLimit: 5,
    });
  });

  it("applies GPUI's history and remove actions", async () => {
    render(<GitHubPanel repoPath={repoPath} gpui />);
    await findGpuiScreen("mergeQueue");

    field("mergeQueue.showMerged", true);
    await findGpuiScreen("mergeQueue", (s) => {
      expect(s.showMergedHistory).toBe(true);
    });
    field("mergeQueue.historyLimit", 10);
    await findGpuiScreen("mergeQueue", (s) => {
      expect(s.historyLimit).toBe(10);
    });
    field("mergeQueue.dequeue", ["base", "top"]);
    expect(dequeue).toHaveBeenCalledWith(["base", "top"]);
  });

  it("opens settings and switches tabs through React", async () => {
    const onOpenSettings = vi.fn();
    render(
      <GitHubPanel repoPath={repoPath} onOpenSettings={onOpenSettings} gpui />,
    );
    await findGpuiScreen("mergeQueue");

    press("mergeQueue.openSettings");
    expect(onOpenSettings).toHaveBeenCalledWith("integrations");
    press("github.tab:prs");
    await waitFor(() => expect(window.location.hash).toBe("#/github/prs/open"));
    await waitFor(() => expect(gpuiScreen()).toBeNull());
  });
});
