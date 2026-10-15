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

/** djb2 hash of diff lines (with their +/-/space prefix), as hex. */
export function hashDiffLines(lines: string[]): string {
  const text = lines.join("\n");
  let hash = 5381;
  for (let i = 0; i < text.length; i++) {
    hash = (hash * 33 + text.charCodeAt(i)) % 4_294_967_296;
  }
  return hash.toString(16).padStart(8, "0");
}

/** Never a real hunk id: the diff viewer lists such a comment as outdated. */
const OUTDATED_HUNK_ID = "__outdated__";

/** The hunk and diff lines `start..end` on `side` cover, if all are present. */
function findCommentedLines(
  hunks: Map<string, FileHunksData>,
  draft: AgentReviewComment,
): { hunkId: string; lines: string[] } | null {
  const side = draft.side ?? "new";
  const lineSide = (n: { old?: number; new?: number }) =>
    n.old !== undefined && n.new === undefined ? "old" : "new";
  for (const hunk of hunks.get(draft.file_path)?.hunks ?? []) {
    const numbers = computeHunkLineNumbers(hunk);
    const lines = hunk.lines.filter((_, i) => {
      const n = numbers[i];
      const num = side === "old" ? n.old : n.new;
      return (
        lineSide(n) === side &&
        num !== undefined &&
        num >= draft.start_line &&
        num <= draft.end_line
      );
    });
    if (lines.length === draft.end_line - draft.start_line + 1) {
      return { hunkId: hunk.id, lines };
    }
  }
  return null;
}

/**
 * A stored PR review draft → the diff viewer's comment. It is outdated when
 * its lines left the diff or no longer match the hash taken when it was
 * written. `lineHash` is the current hash, for drafts stored without one.
 */
export function draftToLineComment(
  draft: AgentReviewComment,
  hunks: Map<string, FileHunksData>,
): { comment: LineComment; lineHash: string | null; outdated: boolean } {
  const found = findCommentedLines(hunks, draft);
  const lineHash = found ? hashDiffLines(found.lines) : null;
  const outdated =
    !found || (draft.line_hash != null && draft.line_hash !== lineHash);
  const text =
    draft.suggested_replacement != null &&
    !draft.comment_text.includes("```suggestion")
      ? `${draft.comment_text}\n\n\`\`\`suggestion\n${draft.suggested_replacement}\n\`\`\``
      : draft.comment_text;
  return {
    comment: {
      id: draft.id,
      filePath: draft.file_path,
      hunkId: outdated ? OUTDATED_HUNK_ID : (found?.hunkId ?? ""),
      startLine: draft.start_line,
      endLine: draft.end_line,
      // An outdated draft must not show the lines that replaced its own.
      lineContent: outdated ? [] : (found?.lines ?? []),
      text,
      createdAt: draft.created_at,
      lineSide: draft.side ?? "new",
    },
    lineHash,
    outdated,
  };
}
