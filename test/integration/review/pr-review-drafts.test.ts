// @include-parallel
import { describe, expect, it } from "vitest";
import {
  createReviewDraft,
  deleteAgentReviewComment,
  listAgentReviewComments,
  resolveAgentReviewComment,
  updateAgentReviewComment,
} from "../../../src/lib/api";
import { createTestRepo } from "../../utils";

describe("PR review drafts", () => {
  it("persist with their line hash and survive edits", async () => {
    const { repoPath } = createTestRepo(false);
    const draft = await createReviewDraft(repoPath, {
      targetType: "github_pr",
      targetId: "42",
      filePath: "src/search.ts",
      startLine: 3,
      endLine: 4,
      side: "new",
      commentText: "Trim first.",
      lineHash: "abc123",
    });
    expect(draft.source).toBe("local-human");

    await updateAgentReviewComment(repoPath, draft.id, {
      commentText: "Trim before matching.",
    });
    const [stored] = await listAgentReviewComments(repoPath, "github_pr", "42");
    expect(stored).toMatchObject({
      id: draft.id,
      comment_text: "Trim before matching.",
      line_hash: "abc123",
      start_line: 3,
      end_line: 4,
      side: "new",
      status: "open",
    });

    await resolveAgentReviewComment(repoPath, draft.id);
    const [resolved] = await listAgentReviewComments(
      repoPath,
      "github_pr",
      "42",
    );
    expect(resolved.status).toBe("resolved");

    await deleteAgentReviewComment(repoPath, draft.id);
    expect(await listAgentReviewComments(repoPath, "github_pr", "42")).toEqual(
      [],
    );
  });
});
