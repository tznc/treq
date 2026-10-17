import { describe, expect, it } from "vitest";
import type { AgentReviewComment } from "./api-types-review";
import {
  commentsToGhReview,
  draftToLineComment,
  hashDiffLines,
  parseUnifiedDiff,
} from "./pr-review";
import type { LineComment } from "./review";

const DIFF = `diff --git a/src/a.ts b/src/a.ts
index 1111111..2222222 100644
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,3 +1,4 @@ export function a() {
 const x = 1;
-const y = 2;
+const y = 3;
+const z = 4;
 export { x };
@@ -10,2 +11,2 @@
-old
+new
 tail
diff --git a/src/new.ts b/src/new.ts
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/src/new.ts
@@ -0,0 +1 @@
+hello
diff --git a/gone.ts b/gone.ts
deleted file mode 100644
--- a/gone.ts
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/old/name.ts b/new/name.ts
similarity index 100%
rename from old/name.ts
rename to new/name.ts
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
`;

describe("parseUnifiedDiff", () => {
  it("parses files with status and hunks", () => {
    const { files, hunks } = parseUnifiedDiff(DIFF);
    expect(files).toEqual([
      { path: "src/a.ts", workspaceStatus: "M" },
      { path: "src/new.ts", workspaceStatus: "A" },
      { path: "gone.ts", workspaceStatus: "D" },
      { path: "new/name.ts", workspaceStatus: "R", oldPath: "old/name.ts" },
      { path: "img.png", workspaceStatus: "M" },
    ]);
    const a = hunks.get("src/a.ts")!;
    expect(a.isLoading).toBe(false);
    expect(a.hunks).toHaveLength(2);
    expect(a.hunks[0].header).toBe("@@ -1,3 +1,4 @@ export function a() {");
    expect(a.hunks[0].lines).toEqual([
      " const x = 1;",
      "-const y = 2;",
      "+const y = 3;",
      "+const z = 4;",
      " export { x };",
    ]);
    expect(a.hunks[1].lines).toEqual(["-old", "+new", " tail"]);
    expect(new Set([a.hunks[0].id, a.hunks[1].id]).size).toBe(2);
    expect(hunks.get("src/new.ts")!.hunks[0].lines).toEqual(["+hello"]);
    expect(hunks.get("new/name.ts")!.hunks).toEqual([]);
  });

  it("drops the no-newline marker and handles CRLF-free empty input", () => {
    const { files, hunks } = parseUnifiedDiff(
      "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n",
    );
    expect(files).toHaveLength(1);
    expect(hunks.get("x")!.hunks[0].lines).toEqual(["-a", "+b"]);
    expect(parseUnifiedDiff("").files).toEqual([]);
  });
});

const comment = (over: Partial<LineComment>): LineComment => ({
  id: "c",
  filePath: "src/a.ts",
  hunkId: "h",
  startLine: 3,
  endLine: 3,
  lineContent: ["x"],
  text: "nit",
  createdAt: "",
  lineSide: "new",
  ...over,
});

describe("commentsToGhReview", () => {
  it("maps line comments to inline review comments", () => {
    const { comments, fileComments } = commentsToGhReview([
      comment({}),
      comment({ startLine: 1, endLine: 4, lineSide: "old", text: "range" }),
      comment({ hunkId: "__file__", text: "whole file" }),
    ]);
    expect(comments).toEqual([
      {
        path: "src/a.ts",
        line: 3,
        side: "RIGHT",
        start_line: null,
        body: "nit",
      },
      { path: "src/a.ts", line: 4, side: "LEFT", start_line: 1, body: "range" },
    ]);
    expect(fileComments).toBe("**src/a.ts**: whole file");
  });
});

const row = (over: Partial<AgentReviewComment> = {}): AgentReviewComment => ({
  id: "a1",
  repo_path: "/r",
  target_type: "github_pr",
  target_id: "7",
  file_path: "src/a.ts",
  hunk_id: null,
  start_line: 11,
  end_line: 11,
  side: "new",
  comment_text: "Rename.",
  suggested_replacement: "newer",
  status: "open",
  source: "local-agent",
  created_at: "",
  resolved_at: null,
  line_hash: null,
  ...over,
});

describe("draftToLineComment", () => {
  const { hunks } = parseUnifiedDiff(DIFF);
  const currentHash = hashDiffLines(["+new"]);

  it("anchors a draft to its hunk and keeps the suggestion", () => {
    const { comment, lineHash, outdated } = draftToLineComment(row(), hunks);
    expect(outdated).toBe(false);
    expect(lineHash).toBe(currentHash);
    expect(comment).toMatchObject({
      id: "a1",
      filePath: "src/a.ts",
      hunkId: "src/a.ts#1",
      startLine: 11,
      endLine: 11,
      lineSide: "new",
      lineContent: ["+new"],
      text: "Rename.\n\n```suggestion\nnewer\n```",
    });
  });

  it("does not repeat a suggestion already written into the text", () => {
    const text = "Edited.\n\n```suggestion\nnewer\n```";
    expect(
      draftToLineComment(row({ comment_text: text }), hunks).comment.text,
    ).toBe(text);
  });

  it("is outdated when the commented lines changed since it was written", () => {
    const stale = draftToLineComment(row({ line_hash: "deadbeef" }), hunks);
    expect(stale.outdated).toBe(true);
    expect(stale.comment.hunkId).not.toBe("src/a.ts#1");
    expect(stale.comment.lineContent).toEqual([]);
  });

  it("shows an outdated draft's original lines, kept in quoted_text", () => {
    const stale = draftToLineComment(
      row({ line_hash: "deadbeef", quoted_text: "-older\n+old" }),
      hunks,
    );
    expect(stale.outdated).toBe(true);
    expect(stale.comment.lineContent).toEqual(["-older", "+old"]);
  });

  it("returns the current lines to store with a draft", () => {
    expect(draftToLineComment(row(), hunks).quotedText).toBe("+new");
    expect(
      draftToLineComment(row({ start_line: 90, end_line: 90 }), hunks)
        .quotedText,
    ).toBeNull();
    expect(
      draftToLineComment(row({ line_hash: currentHash }), hunks).outdated,
    ).toBe(false);
  });

  it("is outdated when its lines are no longer in the diff", () => {
    const gone = draftToLineComment(
      row({ start_line: 90, end_line: 90 }),
      hunks,
    );
    expect(gone.outdated).toBe(true);
    expect(gone.lineHash).toBeNull();
  });
});

describe("draftToLineComment on moved lines", () => {
  const { hunks } = parseUnifiedDiff(DIFF);

  it("follows lines that moved to a new position unchanged", () => {
    const moved = draftToLineComment(
      row({
        start_line: 11,
        end_line: 11,
        line_hash: hashDiffLines(["+const z = 4;"]),
        quoted_text: "+const z = 4;",
      }),
      hunks,
    );
    expect(moved.outdated).toBe(false);
    expect(moved.comment).toMatchObject({
      hunkId: "src/a.ts#0",
      startLine: 3,
      endLine: 3,
      lineContent: ["+const z = 4;"],
    });
  });

  it("follows a multi-line range", () => {
    const moved = draftToLineComment(
      row({
        start_line: 20,
        end_line: 21,
        line_hash: hashDiffLines(["+const y = 3;", "+const z = 4;"]),
      }),
      hunks,
    );
    expect(moved.outdated).toBe(false);
    expect(moved.comment).toMatchObject({ startLine: 2, endLine: 3 });
  });

  it("picks the copy nearest the original line when the lines repeat", () => {
    const { hunks: repeated } = parseUnifiedDiff(
      "diff --git a/r.ts b/r.ts\n--- a/r.ts\n+++ b/r.ts\n@@ -0,0 +1,9 @@\n+x\n+a\n+b\n+c\n+d\n+e\n+f\n+x\n+g\n",
    );
    const moved = draftToLineComment(
      row({
        file_path: "r.ts",
        start_line: 7,
        end_line: 7,
        line_hash: hashDiffLines(["+x"]),
      }),
      repeated,
    );
    expect(moved.comment).toMatchObject({ startLine: 8, endLine: 8 });
  });

  it("does not move a draft whose lines are unchanged in place", () => {
    const { comment } = draftToLineComment(
      row({ line_hash: hashDiffLines(["+new"]) }),
      hunks,
    );
    expect(comment).toMatchObject({ startLine: 11, hunkId: "src/a.ts#1" });
  });
});

describe("hashDiffLines", () => {
  it("is stable and content-sensitive", () => {
    expect(hashDiffLines(["+a", " b"])).toBe(hashDiffLines(["+a", " b"]));
    expect(hashDiffLines(["+a", " b"])).not.toBe(hashDiffLines(["+a", " c"]));
    expect(hashDiffLines(["+ab"])).not.toBe(hashDiffLines(["+a", "b"]));
  });
});
