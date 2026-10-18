import { openUrl } from "@tauri-apps/plugin-opener";
import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, within } from "../../../test/test-utils";
import type { GhReviewThread } from "../../lib/api-types";
import {
  formatGithubCommentDate,
  GithubCommentCard,
  GithubThreadReplyContext,
} from "./GithubCommentCard";

function thread(overrides: Partial<GhReviewThread> = {}): GhReviewThread {
  return {
    id: "PRRT_1",
    is_resolved: false,
    is_outdated: false,
    path: "example.ts",
    line: 2,
    diff_side: "RIGHT",
    comments: [
      {
        id: "PRRC_1",
        body: "Please rename this.",
        author: {
          login: "octocat",
          avatar_url: "https://avatars.githubusercontent.com/u/1?v=4",
        },
        created_at: "2026-08-09T10:00:00Z",
        diff_hunk: "@@ -0,0 +1 @@\n+ const x = 1;",
        url: "https://github.com/treq-dev/treq/pull/1#discussion_r1",
      },
    ],
    ...overrides,
  };
}

describe("formatGithubCommentDate", () => {
  it("omits the year when the date is in the current year", () => {
    const now = new Date();
    const iso = new Date(
      Date.UTC(now.getFullYear(), 7, 9, 12, 0, 0),
    ).toISOString();
    const formatted = formatGithubCommentDate(iso);
    expect(formatted).not.toMatch(String(now.getFullYear()));
    expect(formatted).toMatch(/9/);
    expect(formatted).toMatch(/Aug/i);
  });

  it("includes the year when the date is not in the current year", () => {
    const formatted = formatGithubCommentDate("2024-08-09T12:00:00Z");
    expect(formatted).toMatch(/2024/);
    expect(formatted).toMatch(/9/);
    expect(formatted).toMatch(/Aug/i);
  });
});

describe("GithubCommentCard", () => {
  it("shows the avatar and a GitHub profile link for the username", async () => {
    const user = userEvent.setup();
    vi.mocked(openUrl).mockClear();

    render(
      <GithubCommentCard
        thread={thread()}
        collapsed={false}
        onToggleCollapse={() => {}}
        onQuote={() => {}}
      />,
    );

    expect(screen.getByRole("img", { name: "octocat" })).toBeInTheDocument();

    const profileLink = screen.getByRole("link", { name: "@octocat" });
    expect(profileLink).toHaveAttribute("href", "https://github.com/octocat");
    await user.click(profileLink);
    expect(openUrl).toHaveBeenCalledWith("https://github.com/octocat");
  });

  it("shows the GitHub icon on each comment, not on the collapsed thread header", () => {
    const { rerender } = render(
      <GithubCommentCard
        thread={thread()}
        collapsed={true}
        onToggleCollapse={() => {}}
        onQuote={() => {}}
      />,
    );

    const toggle = screen.getByTestId("github-thread-toggle");
    expect(within(toggle).queryByLabelText("GitHub")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("GitHub")).not.toBeInTheDocument();

    rerender(
      <GithubCommentCard
        thread={thread()}
        collapsed={false}
        onToggleCollapse={() => {}}
        onQuote={() => {}}
      />,
    );

    expect(within(toggle).queryByLabelText("GitHub")).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId("github-comment-card")).getByLabelText(
        "GitHub",
      ),
    ).toBeInTheDocument();
  });

  it("shows avatar, username, and a first-comment preview when collapsed", () => {
    const longBody =
      "First line of feedback that should remain visible.\nSecond line that should also peek through.\nThird line that should be clamped away when collapsed.";
    render(
      <GithubCommentCard
        thread={thread({
          is_resolved: true,
          comments: [
            {
              id: "PRRC_1",
              body: longBody,
              author: {
                login: "octocat",
                avatar_url: "https://avatars.githubusercontent.com/u/1?v=4",
              },
              created_at: "2026-08-09T10:00:00Z",
              diff_hunk: "@@ -0,0 +1 @@\n+ const x = 1;",
              url: "https://github.com/treq-dev/treq/pull/1#discussion_r1",
            },
          ],
        })}
        collapsed={true}
        onToggleCollapse={() => {}}
        onQuote={() => {}}
      />,
    );

    const toggle = screen.getByTestId("github-thread-toggle");
    expect(
      within(toggle).getByRole("img", { name: "octocat" }),
    ).toBeInTheDocument();
    expect(
      within(toggle).getByRole("link", { name: "@octocat" }),
    ).toBeInTheDocument();
    expect(within(toggle).getByText("Resolved")).toBeInTheDocument();

    const preview = screen.getByTestId("github-thread-collapsed-preview");
    expect(preview).toHaveTextContent(
      "First line of feedback that should remain visible.",
    );
    // Preview sits on the same header row, after the Resolved badge.
    const headerRow = Array.from(toggle.childNodes).filter(
      (node) => node.nodeType === Node.ELEMENT_NODE,
    ) as Element[];
    const resolvedIndex = headerRow.findIndex((el) =>
      el.textContent?.includes("Resolved"),
    );
    const previewIndex = headerRow.findIndex(
      (el) =>
        el.getAttribute("data-testid") === "github-thread-collapsed-preview",
    );
    expect(resolvedIndex).toBeGreaterThanOrEqual(0);
    expect(previewIndex).toBeGreaterThan(resolvedIndex);
    expect(screen.queryByTestId("github-comment-card")).not.toBeInTheDocument();
  });

  it("falls back to an initial avatar when the author has no avatar_url while collapsed", () => {
    render(
      <GithubCommentCard
        thread={thread({
          is_resolved: true,
          comments: [
            {
              id: "PRRC_1",
              body: "Looks good.",
              author: { login: "reviewer-bot", avatar_url: null },
              created_at: "2026-08-09T10:00:00Z",
              diff_hunk: "@@ -0,0 +1 @@\n+ const x = 1;",
              url: "https://github.com/treq-dev/treq/pull/1#discussion_r1",
            },
          ],
        })}
        collapsed={true}
        onToggleCollapse={() => {}}
        onQuote={() => {}}
      />,
    );

    const toggle = screen.getByTestId("github-thread-toggle");
    expect(within(toggle).queryByRole("img")).not.toBeInTheDocument();
    expect(within(toggle).getByText("R")).toBeInTheDocument();
    expect(
      within(toggle).getByRole("link", { name: "@reviewer-bot" }),
    ).toBeInTheDocument();
  });
});

describe("GithubCommentCard replies", () => {
  it("has no reply box without a reply handler", () => {
    render(
      <GithubCommentCard
        thread={thread()}
        collapsed={false}
        onToggleCollapse={() => {}}
        onQuote={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: "Reply" })).toBeNull();
  });

  it("posts a reply to the thread and clears the box", async () => {
    const user = userEvent.setup();
    const reply = vi.fn().mockResolvedValue(undefined);
    render(
      <GithubThreadReplyContext.Provider value={reply}>
        <GithubCommentCard
          thread={thread()}
          collapsed={false}
          onToggleCollapse={() => {}}
          onQuote={() => {}}
        />
      </GithubThreadReplyContext.Provider>,
    );
    await user.click(screen.getByRole("button", { name: "Reply" }));
    const box = screen.getByPlaceholderText("Reply on GitHub…");
    await user.type(box, "Done, renamed.");
    await user.click(screen.getByRole("button", { name: "Send reply" }));
    expect(reply).toHaveBeenCalledWith("PRRT_1", "Done, renamed.");
    expect(screen.queryByPlaceholderText("Reply on GitHub…")).toBeNull();
  });
});
