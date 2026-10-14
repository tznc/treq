import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { render, screen, waitFor } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";

// The PR lives on GitHub, so the `gh`-backed calls are stubbed at the
// src/lib/api boundary. The repo, Dashboard and everything else stay real.
const gh = vi.hoisted(() => ({
  getGitRemoteUrl: vi.fn(),
  ghListPrs: vi.fn(),
  ghListIssues: vi.fn(),
  ghViewPr: vi.fn(),
  ghPrDiff: vi.fn(),
  ghListPrReviewThreads: vi.fn(),
  ghReplyPrReviewThread: vi.fn(),
  ghSubmitPrReview: vi.fn(),
  listAgentReviewComments: vi.fn(),
  resolveAgentReviewComment: vi.fn(),
  getPrChecksForPr: vi.fn(),
}));

vi.mock("../../../src/lib/api", async () => ({
  ...(await vi.importActual<typeof import("../../../src/lib/api")>(
    "../../../src/lib/api",
  )),
  ...gh,
  startPrStatusPolling: vi.fn(async () => undefined),
  stopPrStatusPolling: vi.fn(async () => undefined),
}));

const PR = {
  number: 42,
  title: "Trim search queries before matching",
  state: "OPEN",
  url: "https://github.com/acme/search/pull/42",
  body: "Trims whitespace from the query.",
  author: { login: "alice" },
  labels: [],
  head_ref_name: "feat/trim-search",
  base_ref_name: "main",
  merge_state_status: "CLEAN",
  created_at: "2026-10-01T10:00:00Z",
  updated_at: "2026-10-01T10:00:00Z",
  comments: null,
  is_draft: false,
};

const DIFF = `diff --git a/src/search.ts b/src/search.ts
index 1111111..2222222 100644
--- a/src/search.ts
+++ b/src/search.ts
@@ -1,6 +1,8 @@
 import { index } from "./index";
 
 export function search(q: string) {
-  return index.filter((item) => item.name.includes(q));
+  const query = q.trim().toLowerCase();
+  if (!query) return [];
+  return index.filter((item) => item.name.includes(q));
 }
 
diff --git a/src/search.test.ts b/src/search.test.ts
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/src/search.test.ts
@@ -0,0 +1,5 @@
+import { search } from "./search";
+
+it("ignores surrounding whitespace", () => {
+  expect(search("  foo ")).toEqual(search("foo"));
+});
`;

it("reviews a GitHub PR from its own screen", async () => {
  gh.getGitRemoteUrl.mockResolvedValue({
    owner: "acme",
    repo: "search",
    full_name: "acme/search",
  });
  gh.ghListPrs.mockResolvedValue({ items: [PR], hasMore: false });
  gh.ghListIssues.mockResolvedValue({ items: [], hasMore: false });
  gh.ghViewPr.mockResolvedValue(PR);
  gh.ghPrDiff.mockResolvedValue(DIFF);
  gh.getPrChecksForPr.mockResolvedValue(null);
  gh.ghListPrReviewThreads.mockResolvedValue([
    {
      id: "PRRT_1",
      is_resolved: false,
      is_outdated: false,
      path: "src/search.ts",
      line: 4,
      diff_side: "RIGHT",
      comments: [
        {
          id: "PRRC_1",
          body: "Should empty queries return everything instead?",
          author: { login: "octocat", avatar_url: null },
          created_at: "2026-10-02T09:00:00Z",
          diff_hunk: "",
          url: "https://github.com/acme/search/pull/42#discussion_r1",
        },
      ],
    },
  ]);
  let agentStatus = "open";
  gh.listAgentReviewComments.mockImplementation(async () => [
    {
      id: "agent-1",
      repo_path: "",
      target_type: "github_pr",
      target_id: "42",
      file_path: "src/search.ts",
      hunk_id: null,
      start_line: 6,
      end_line: 6,
      side: "new",
      comment_text:
        "The filter still matches on `q`, so the trimmed `query` is never used.",
      suggested_replacement:
        "  return index.filter((item) => item.name.toLowerCase().includes(query));",
      status: agentStatus,
      source: "local-agent",
      created_at: "",
      resolved_at: null,
    },
  ]);
  gh.resolveAgentReviewComment.mockImplementation(async () => {
    agentStatus = "resolved";
  });
  gh.ghReplyPrReviewThread.mockResolvedValue(undefined);
  gh.ghSubmitPrReview.mockResolvedValue(undefined);

  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const user = userEvent.setup();
  render(<Dashboard />);

  await user.click(await screen.findByTestId("github-sidebar-item"));
  await user.click(await screen.findByRole("tab", { name: /Pull Requests/i }));
  await user.click(await screen.findByText(PR.title));
  await user.click(
    await screen.findByRole("button", { name: /Review changes/ }),
  );
  await screen.findByTestId("pr-review-page");
  await screen.findByText("Should empty queries return everything instead?");
  await screen.findByText(/trimmed `query` is never used/);
  await captureDocument(document, {
    name: "github-pr-review-01-screen",
    expectations: [
      "A full-screen PR review shows the PR title, a changed-files list on the left, and the PR diff with + / - lines.",
      "octocat's GitHub thread is shown inline under its diff line with a Reply link, and a violet Local agent comment shows an Add to review button.",
      'The header has "AI review" and "Finish review" buttons.',
    ],
  });

  await user.click(screen.getByRole("button", { name: "Reply" }));
  await user.type(
    screen.getByPlaceholderText("Reply on GitHub…"),
    "Empty should return nothing; the search box hides results anyway.",
  );
  await captureDocument(document, {
    name: "github-pr-review-02-reply",
    expectations: [
      "A reply box under octocat's thread holds the typed reply, with Cancel and Send reply buttons.",
    ],
  });
  await user.click(screen.getByRole("button", { name: "Send reply" }));
  await waitFor(() =>
    expect(gh.ghReplyPrReviewThread).toHaveBeenCalledWith(
      "PRRT_1",
      "Empty should return nothing; the search box hides results anyway.",
    ),
  );

  await user.click(screen.getByRole("button", { name: /Add to review/ }));
  await user.click(
    await screen.findByRole("button", { name: /Finish review \(1\)/ }),
  );
  await user.type(
    await screen.findByPlaceholderText("Leave a comment"),
    "Nice cleanup, one fix needed.",
  );
  await user.click(screen.getByRole("radio", { name: "Request changes" }));
  await captureDocument(document, {
    name: "github-pr-review-03-finish-popover",
    expectations: [
      'A "Finish your review" popover says 1 pending comment will be posted and shows the typed summary.',
      'Comment / Approve / Request changes radio options are listed, with "Request changes" selected, plus a Submit review button.',
      "The promoted agent comment shows as a draft comment (with its suggestion block) in the diff, and the violet Local card is gone.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Submit review" }));
  await waitFor(() =>
    expect(gh.ghSubmitPrReview).toHaveBeenCalledWith(
      "acme/search",
      42,
      "REQUEST_CHANGES",
      "Nice cleanup, one fix needed.",
      [expect.objectContaining({ path: "src/search.ts", line: 6, side: "RIGHT" })],
    ),
  );
}, 90000);
