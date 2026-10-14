import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { PrReviewScreen } from "./PrReviewPage";

const api = vi.hoisted(() => ({
  getGitRemoteUrl: vi.fn(),
  ghViewPr: vi.fn(),
  ghPrDiff: vi.fn(),
  ghListPrReviewThreads: vi.fn(),
  ghReplyPrReviewThread: vi.fn(),
  ghSubmitPrReview: vi.fn(),
  listAgentReviewComments: vi.fn(),
  resolveAgentReviewComment: vi.fn(),
}));

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  ...api,
}));

const DIFF = `diff --git a/src/search.ts b/src/search.ts
index 1..2 100644
--- a/src/search.ts
+++ b/src/search.ts
@@ -1,2 +1,3 @@
 export function search(q: string) {
+  const query = q.trim();
   return q;
`;

const THREAD = {
  id: "PRRT_1",
  is_resolved: false,
  is_outdated: false,
  path: "src/search.ts",
  line: 2,
  diff_side: "RIGHT",
  comments: [
    {
      id: "PRRC_1",
      body: "Use the trimmed query below.",
      author: { login: "octocat", avatar_url: null },
      created_at: "2026-08-09T10:00:00Z",
      diff_hunk: "",
      url: "https://github.com/acme/treq/pull/42#discussion_r1",
    },
  ],
};

const AGENT_COMMENT = {
  id: "agent-1",
  repo_path: "/repo",
  target_type: "github_pr",
  target_id: "42",
  file_path: "src/search.ts",
  hunk_id: null,
  start_line: 3,
  end_line: 3,
  side: "new",
  comment_text: "Returns the untrimmed query.",
  suggested_replacement: "  return query;",
  status: "open",
  source: "local-agent",
  created_at: "",
  resolved_at: null,
};

const diffLineTexts = () =>
  Array.from(document.querySelectorAll("[data-diff-line]"), (el) =>
    (el.textContent ?? "").replace(/^\s*\d*\s*\d*\s*[+ -]?/, ""),
  );

function renderScreen(onStartAgentReview = vi.fn()) {
  render(
    <PrReviewScreen
      repoPath="/repo"
      prNumber={42}
      onClose={() => {}}
      onStartAgentReview={onStartAgentReview}
    />,
  );
}

describe("PR review screen", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    vi.clearAllMocks();
    user = userEvent.setup();
    api.getGitRemoteUrl.mockResolvedValue({
      owner: "acme",
      repo: "treq",
      full_name: "acme/treq",
    });
    api.ghViewPr.mockResolvedValue({
      number: 42,
      title: "Trim search queries",
      state: "OPEN",
      url: "https://github.com/acme/treq/pull/42",
      body: "",
      author: { login: "alice" },
      labels: [],
      head_ref_name: "feat/search",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      created_at: "",
      updated_at: "",
      comments: null,
    });
    api.ghPrDiff.mockResolvedValue(DIFF);
    api.ghListPrReviewThreads.mockResolvedValue([THREAD]);
    api.ghReplyPrReviewThread.mockResolvedValue(undefined);
    api.ghSubmitPrReview.mockResolvedValue(undefined);
    api.listAgentReviewComments.mockResolvedValue([AGENT_COMMENT]);
    api.resolveAgentReviewComment.mockResolvedValue(undefined);
  });

  it("shows the PR diff with its GitHub threads", async () => {
    renderScreen();
    expect(await screen.findByText("Trim search queries")).toBeTruthy();
    await waitFor(() =>
      expect(diffLineTexts()).toContain("  const query = q.trim();"),
    );
    expect(
      await screen.findByText("Use the trimmed query below."),
    ).toBeTruthy();
  });

  it("replies to an existing thread", async () => {
    renderScreen();
    await user.click(await screen.findByRole("button", { name: "Reply" }));
    await user.type(
      screen.getByPlaceholderText("Reply on GitHub…"),
      "Done in the next push",
    );
    await user.click(screen.getByRole("button", { name: "Send reply" }));
    expect(api.ghReplyPrReviewThread).toHaveBeenCalledWith(
      "PRRT_1",
      "Done in the next push",
    );
  });

  it("starts an AI review of the PR", async () => {
    const onStart = vi.fn().mockResolvedValue(undefined);
    renderScreen(onStart);
    await waitFor(() => expect(diffLineTexts().length).toBeGreaterThan(0));
    await user.click(screen.getByRole("button", { name: /AI review/ }));
    await waitFor(() => expect(onStart).toHaveBeenCalled());
    const [[prompt]] = onStart.mock.calls;
    expect(prompt).toContain("--target-type github_pr");
    expect(prompt).toContain("--target-id 42");
    expect(prompt).toContain("gh pr diff 42 --repo acme/treq");
  });

  it("approves with an agent comment promoted into the review", async () => {
    renderScreen();
    await user.click(
      await screen.findByRole("button", { name: /Add to review/ }),
    );
    expect(api.resolveAgentReviewComment).toHaveBeenCalledWith(
      "/repo",
      "agent-1",
    );

    await user.click(screen.getByRole("button", { name: /Finish review/ }));
    const popover = await screen.findByText("Finish your review");
    const panel = popover.closest("[role='dialog']") ?? document.body;
    expect(
      within(panel as HTMLElement).getByText(/1 pending comment/),
    ).toBeTruthy();
    await user.type(screen.getByPlaceholderText("Leave a comment"), "LGTM");
    await user.click(screen.getByRole("radio", { name: "Approve" }));
    await user.click(screen.getByRole("button", { name: "Submit review" }));

    await waitFor(() =>
      expect(api.ghSubmitPrReview).toHaveBeenCalledWith(
        "acme/treq",
        42,
        "APPROVE",
        "LGTM",
        [
          {
            path: "src/search.ts",
            line: 3,
            side: "RIGHT",
            start_line: null,
            body: "Returns the untrimmed query.\n\n```suggestion\n  return query;\n```",
          },
        ],
      ),
    );
  });
});
