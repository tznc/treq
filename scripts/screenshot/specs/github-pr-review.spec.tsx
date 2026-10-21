import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { PrReviewScreen } from "../../../src/components/github-panel/PrReviewPage";
import { render, screen, waitFor } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { invalidateQueries } from "../../../src/lib/swr-cache";
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
  deleteAgentReviewComment: vi.fn(),
  ghCurrentUser: vi.fn(),
  createReviewDraft: vi.fn(),
  updateAgentReviewComment: vi.fn(),
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

// The desktop app draws this screen in GPUI for local repos; the React
// screen (used for remote repos) is rendered directly. "Back" leaves it and
// "Review changes" reopens it, like the PR detail does.
function ReviewHarness({ repoPath }: { repoPath: string }) {
  const [open, setOpen] = React.useState(true);
  return open ? (
    <PrReviewScreen
      repoPath={repoPath}
      prNumber={42}
      onClose={() => setOpen(false)}
      onStartAgentReview={async () => {}}
    />
  ) : (
    <button type="button" onClick={() => setOpen(true)}>
      Review changes
    </button>
  );
}

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
  // Stands in for the local DB the drafts persist in.
  type Draft = Record<string, unknown> & { id: string };
  let drafts: Draft[] = [];
  const row = (over: Partial<Draft> & { id: string }): Draft => ({
    repo_path: "",
    target_type: "github_pr",
    target_id: "42",
    hunk_id: null,
    suggested_replacement: null,
    status: "open",
    created_at: "",
    resolved_at: null,
    line_hash: null,
    ...over,
  });
  gh.listAgentReviewComments.mockImplementation(async () =>
    drafts.map((d) => ({ ...d })),
  );
  gh.createReviewDraft.mockImplementation(async (_repo, input) => {
    const draft = row({
      id: `human-${drafts.length}`,
      file_path: input.filePath,
      start_line: input.startLine,
      end_line: input.endLine,
      side: input.side,
      comment_text: input.commentText,
      line_hash: input.lineHash,
      quoted_text: input.quotedText,
      source: "local-human",
    });
    drafts.push(draft);
    return draft;
  });
  gh.updateAgentReviewComment.mockImplementation(async (_repo, id, update) => {
    drafts = drafts.map((d) =>
      d.id !== id
        ? d
        : {
            ...d,
            comment_text: update.commentText ?? d.comment_text,
            line_hash: update.lineHash ?? d.line_hash,
            quoted_text: update.quotedText ?? d.quoted_text,
          },
    );
  });
  gh.resolveAgentReviewComment.mockImplementation(async (_repo, id) => {
    drafts = drafts.map((d) =>
      d.id === id ? { ...d, status: "resolved" } : d,
    );
  });
  gh.ghCurrentUser.mockResolvedValue("bob");
  gh.ghReplyPrReviewThread.mockResolvedValue(undefined);
  gh.ghSubmitPrReview.mockResolvedValue(undefined);

  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const user = userEvent.setup();
  render(<ReviewHarness repoPath={repoPath} />);
  await screen.findByTestId("pr-review-page");
  await screen.findByText("Should empty queries return everything instead?");
  await captureDocument(document, {
    name: "github-pr-review-01-screen",
    expectations: [
      "A full-screen PR review shows the PR title, a changed-files list on the left, and the PR diff with + / - lines.",
      "octocat's GitHub thread is shown inline under its diff line with a Reply link.",
      'The header has "AI review" and "Finish review" buttons, and there are no draft comments yet.',
    ],
  });

  // The AI review agent records a comment; the screen's poll picks it up.
  drafts.push(
    row({
      id: "agent-1",
      file_path: "src/search.ts",
      start_line: 6,
      end_line: 6,
      side: "new",
      comment_text:
        "The filter still matches on `q`, so the trimmed `query` is never used.",
      suggested_replacement:
        "  return index.filter((item) => item.name.toLowerCase().includes(query));",
      source: "local-agent",
    }),
  );
  await invalidateQueries([
    "agent-review-comments",
    repoPath,
    "github_pr",
    "42",
  ]);
  await screen.findByTestId("inline-comment-card");
  await screen.findByRole("button", { name: /Finish review \(1\)/ });
  await captureDocument(document, {
    name: "github-pr-review-02-agent-draft",
    expectations: [
      "Without any navigation, the agent's comment appears as a draft comment card under line 6, including its ```suggestion block.",
      "The draft comment card text is in the sans-serif UI font, not the monospace diff font.",
      'The header button reads "Finish review (1)".',
    ],
  });

  // The agent's draft got the hash of the line it covers.
  await waitFor(() => expect(drafts[0].line_hash).toBeTruthy());

  // The user drafts their own comment on line 4.
  const line4 = Array.from(document.querySelectorAll("[data-diff-line]")).find(
    (el) => el.textContent?.includes("toLowerCase();"),
  ) as HTMLElement;
  await user.click(
    line4.querySelector("[data-testid='line-gutter']") as HTMLElement,
  );
  await user.click(line4.querySelector("[data-comment-button]") as HTMLElement);
  await user.type(
    await screen.findByPlaceholderText(/add a comment/i),
    "Lower-casing here changes matching for mixed-case names.",
  );
  await user.keyboard("{Meta>}{Enter}{/Meta}");
  await screen.findByRole("button", { name: /Finish review \(2\)/ });

  // Leave the screen and come back: both drafts are still there.
  await user.click(screen.getByRole("button", { name: "Back" }));
  await user.click(
    await screen.findByRole("button", { name: /Review changes/ }),
  );
  await screen.findByRole("button", { name: /Finish review \(2\)/ });
  expect(await screen.findAllByTestId("inline-comment-card")).toHaveLength(2);
  await captureDocument(document, {
    name: "github-pr-review-03-drafts-after-navigation",
    expectations: [
      "After leaving the PR review screen and reopening it, two draft comment cards are shown: the user's under line 4 and the agent's under line 6.",
      'The header button reads "Finish review (2)".',
    ],
  });

  // The author pushes: a new line above line 4 moves the user's line down
  // unchanged, and line 6 itself changes, so the agent's draft is outdated.
  gh.ghPrDiff.mockResolvedValue(
    DIFF.replace("@@ -1,6 +1,8 @@", "@@ -1,6 +1,9 @@")
      .replace(
        "+  const query",
        "+  // Normalise before matching.\n+  const query",
      )
      .replace(
        "+  return index.filter((item) => item.name.includes(q));",
        "+  return index.filter((item) => item.name.startsWith(query));",
      ),
  );
  await invalidateQueries(["gh-pr-diff", "acme/search", 42]);
  await screen.findByText("Outdated");
  await screen.findByRole("button", { name: /Finish review \(1\)/ });
  await captureDocument(document, {
    name: "github-pr-review-04-outdated-after-update",
    expectations: [
      "The diff has a new `// Normalise before matching.` line 4, and the agent's draft is in an amber Outdated box above the file's diff.",
      "The Outdated box shows the original line it was written on, `+ return index.filter((item) => item.name.includes(q));`, above the comment and its suggestion.",
      'The user\'s draft followed its line: it is shown inline under `const query = …` (now line 5), and the header button reads "Finish review (1)".',
    ],
  });

  await user.click(screen.getByRole("button", { name: "Reply" }));
  await user.type(
    screen.getByPlaceholderText("Reply on GitHub…"),
    "Empty should return nothing; the search box hides results anyway.",
  );
  await captureDocument(document, {
    name: "github-pr-review-05-reply",
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

  await user.click(
    await screen.findByRole("button", { name: /Finish review \(1\)/ }),
  );
  await user.type(
    await screen.findByPlaceholderText("Leave a comment"),
    "Nice cleanup, one fix needed.",
  );
  await user.click(screen.getByRole("radio", { name: "Request changes" }));
  await captureDocument(document, {
    name: "github-pr-review-06-finish-popover",
    expectations: [
      'A "Finish your review" popover says 1 pending comment will be posted and 1 outdated draft stays as a draft, and shows the typed summary.',
      'Comment / Approve / Request changes radio options are listed, with "Request changes" selected, plus a Submit review button.',
      "The user's draft comment is still shown in the diff.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Submit review" }));
  await waitFor(() =>
    expect(gh.ghSubmitPrReview).toHaveBeenCalledWith(
      "acme/search",
      42,
      "REQUEST_CHANGES",
      "Nice cleanup, one fix needed.",
      [
        expect.objectContaining({
          path: "src/search.ts",
          line: 5,
          side: "RIGHT",
        }),
      ],
    ),
  );
}, 90000);

it("limits your own PR's review to Comment", async () => {
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
  gh.ghListPrReviewThreads.mockResolvedValue([]);
  gh.listAgentReviewComments.mockResolvedValue([]);
  gh.ghCurrentUser.mockResolvedValue(PR.author.login);

  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<ReviewHarness repoPath={repoPath} />);
  await user.click(
    await screen.findByRole("button", { name: /Finish review/ }),
  );
  await waitFor(() =>
    expect(screen.getByRole("radio", { name: "Approve" })).toBeDisabled(),
  );
  expect(screen.getByRole("radio", { name: "Request changes" })).toBeDisabled();
  await captureDocument(document, {
    name: "github-pr-review-07-own-pr",
    expectations: [
      'In the "Finish your review" popover, "Approve" and "Request changes" radios are disabled (greyed out) and explain that PR authors cannot approve or request changes on their own PR.',
      '"Comment" stays selectable and selected.',
    ],
  });
}, 90000);
