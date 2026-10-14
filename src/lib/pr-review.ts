// Turns a GitHub PR's unified diff into the shapes the Review-tab diff viewer
// renders, and turns local review comments back into a GitHub review payload.
import type { FileHunksData } from "../components/changes-diff-viewer/types";
import { computeHunkLineNumbers } from "../components/diff/hunk-lines";
import type { GhDraftReviewComment } from "./api-github";
import type { AgentReviewComment } from "./api-types-review";
import type { JjDiffHunk } from "./api-types";
import type { ParsedFileChange } from "./git-utils";
import { FILE_COMMENT_HUNK_ID, type LineComment } from "./review";

const stripPrefix = (path: string): string => path.replace(/^[ab]\//, "");

/** Parse `gh pr diff` output into changed files and per-file hunks. */
export function parseUnifiedDiff(diff: string): {
  files: ParsedFileChange[];
  hunks: Map<string, FileHunksData>;
} {
  const files: ParsedFileChange[] = [];
  const hunks = new Map<string, FileHunksData>();
  let file: ParsedFileChange | null = null;
  let fileHunks: JjDiffHunk[] = [];
  let hunk: JjDiffHunk | null = null;

  const flush = () => {
    if (!file) return;
    files.push(file);
    hunks.set(file.path, {
      filePath: file.path,
      hunks: fileHunks,
      isLoading: false,
    });
  };

  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      flush();
      const match = line.match(/^diff --git a\/(.*) b\/(.*)$/);
      file = { path: match?.[2] ?? line.slice(11), workspaceStatus: "M" };
      fileHunks = [];
      hunk = null;
      continue;
    }
    if (!file) continue;
    if (line.startsWith("@@")) {
      hunk = {
        id: `${file.path}#${fileHunks.length}`,
        header: line,
        lines: [],
        patch: "",
      };
      fileHunks.push(hunk);
    } else if (hunk) {
      if (/^[ +-]/.test(line)) hunk.lines.push(line);
    } else if (line.startsWith("new file mode")) {
      file.workspaceStatus = "A";
    } else if (line.startsWith("deleted file mode")) {
      file.workspaceStatus = "D";
    } else if (line.startsWith("rename from ")) {
      file.workspaceStatus = "R";
      file.oldPath = line.slice("rename from ".length);
    } else if (line.startsWith("rename to ")) {
      file.path = line.slice("rename to ".length);
    } else if (line.startsWith("--- ") && line !== "--- /dev/null") {
      if (file.workspaceStatus === "D") file.path = stripPrefix(line.slice(4));
    } else if (line.startsWith("+++ ") && line !== "+++ /dev/null") {
      file.path = stripPrefix(line.slice(4));
    }
  }
  flush();
  return { files, hunks };
}

/**
 * Local comments → GitHub review comments. Line comments become inline
 * comments; file-level comments have no line, so they go in the review body.
 */
export function commentsToGhReview(comments: LineComment[]): {
  comments: GhDraftReviewComment[];
  fileComments: string;
} {
  const inline: GhDraftReviewComment[] = [];
  const fileLevel: string[] = [];
  for (const c of comments) {
    if (c.hunkId === FILE_COMMENT_HUNK_ID) {
      fileLevel.push(`**${c.filePath}**: ${c.text}`);
      continue;
    }
    inline.push({
      path: c.filePath,
      line: c.endLine,
      side: c.lineSide === "old" ? "LEFT" : "RIGHT",
      start_line: c.startLine < c.endLine ? c.startLine : null,
      body: c.text,
    });
  }
  return { comments: inline, fileComments: fileLevel.join("\n\n") };
}

/**
 * Agent review comment → draft review comment, anchored to the hunk holding
 * its last line. A suggestion becomes a GitHub ```suggestion block.
 */
export function agentCommentToLineComment(
  comment: AgentReviewComment,
  hunks: Map<string, FileHunksData>,
): LineComment {
  const side = comment.side ?? "new";
  let hunkId = "";
  let lineContent: string[] = [];
  for (const hunk of hunks.get(comment.file_path)?.hunks ?? []) {
    const numbers = computeHunkLineNumbers(hunk);
    const lineSide = (n: { old?: number; new?: number }) =>
      n.old !== undefined && n.new === undefined ? "old" : "new";
    const inRange = (n: { old?: number; new?: number }) => {
      const num = side === "old" ? n.old : n.new;
      return (
        lineSide(n) === side &&
        num !== undefined &&
        num >= comment.start_line &&
        num <= comment.end_line
      );
    };
    if (!numbers.some(inRange)) continue;
    hunkId = hunk.id;
    lineContent = hunk.lines.filter((_, i) => inRange(numbers[i]));
    break;
  }
  const text =
    comment.suggested_replacement != null
      ? `${comment.comment_text}\n\n\`\`\`suggestion\n${comment.suggested_replacement}\n\`\`\``
      : comment.comment_text;
  return {
    id: comment.id,
    filePath: comment.file_path,
    hunkId,
    startLine: comment.start_line,
    endLine: comment.end_line,
    lineContent,
    text,
    createdAt: new Date().toISOString(),
    lineSide: side,
  };
}
