import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen } from "../../../test/test-utils";
import type { AgentReviewComment } from "../../lib/api-types-review";
import { AgentReviewCommentCard } from "./AgentReviewCommentCard";

const comment: AgentReviewComment = {
  id: "c1",
  repo_path: "/repo",
  target_type: "github_pr",
  target_id: "7",
  file_path: "src/a.ts",
  hunk_id: null,
  start_line: 3,
  end_line: 3,
  side: "new",
  comment_text: "Off by one.",
  suggested_replacement: "i < n",
  status: "open",
  source: "local-agent",
  created_at: "",
  resolved_at: null,
};

const handlers = () => ({
  onResolve: vi.fn().mockResolvedValue(undefined),
  onDelete: vi.fn().mockResolvedValue(undefined),
  onApplySuggestion: vi.fn().mockResolvedValue(undefined),
  onError: vi.fn(),
});

describe("AgentReviewCommentCard", () => {
  it("offers Add to review instead of Apply when the host can draft review comments", async () => {
    const user = userEvent.setup();
    const onAddToReview = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentReviewCommentCard
        comment={comment}
        {...handlers()}
        onAddToReview={onAddToReview}
      />,
    );
    expect(screen.queryByRole("button", { name: /Apply/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: /Add to review/ }));
    expect(onAddToReview).toHaveBeenCalledWith(comment);
  });

  it("keeps Apply without an Add to review handler", () => {
    render(<AgentReviewCommentCard comment={comment} {...handlers()} />);
    expect(screen.getByRole("button", { name: /Apply/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Add to review/ })).toBeNull();
  });
});
