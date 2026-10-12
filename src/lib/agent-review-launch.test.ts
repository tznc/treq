import { describe, expect, it } from "vitest";
import { autoReviewSummary } from "./agent-review-launch";
import { buildLinearReviewPrompt } from "./agent-review-prompt";

describe("autoReviewSummary", () => {
  it("names the operation that started the review", () => {
    expect(autoReviewSummary("on-commit", "feature")).toContain(
      "the commit just created",
    );
    expect(autoReviewSummary("on-rebase", "feature")).toContain(
      "the rebase just applied",
    );
    expect(autoReviewSummary("on-pull", "feature")).toContain(
      "pulled from the remote",
    );
  });

  it("names the branch when there is one", () => {
    expect(autoReviewSummary("on-commit", "feature")).toContain(
      "the feature workspace diff",
    );
    expect(autoReviewSummary("on-commit", undefined)).toContain(
      "the current workspace diff",
    );
  });
});

describe("buildLinearReviewPrompt", () => {
  it("points the agent at the snapshot and the Linear target", () => {
    const prompt = buildLinearReviewPrompt({
      targetType: "linear_issue",
      targetId: "issue-1",
      diffSummary: 'the Linear issue "ENG-1 Ship search"',
      snapshotDir: "/repo/.treq/linear-review/linear_issue/issue-1",
    });
    expect(prompt).toContain(
      "copied the text under review into /repo/.treq/linear-review/linear_issue/issue-1",
    );
    expect(prompt).toContain(
      "treq agent-review list --target-type linear_issue --target-id issue-1",
    );
    expect(prompt).toContain("--file <body.md or comments/<id>.md>");
    expect(prompt).toContain("\\`\\`\\`suggestion");
    expect(prompt).not.toMatch(
      /\{(target_type|target_id|snapshot_dir|diff_summary)\}/,
    );
  });
});
