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
 * Where a draft's lines moved to: the run of diff lines on its side that
 * hashes to `line_hash`, nearest its original start. Only lines in the diff
 * can be found, so a range that moved outside every hunk stays lost.
 */
function findMovedLines(
  hunks: Map<string, FileHunksData>,
  draft: AgentReviewComment,
  lineHash: string,
): { hunkId: string; lines: string[]; startLine: number } | null {
  const side = draft.side ?? "new";
  const size = draft.end_line - draft.start_line + 1;
  let best: { hunkId: string; lines: string[]; startLine: number } | null =
    null;
  for (const hunk of hunks.get(draft.file_path)?.hunks ?? []) {
    const numbers = computeHunkLineNumbers(hunk);
    // Consecutive lines on one side of a hunk have consecutive numbers.
    const sideLines = hunk.lines
      .map((text, i) => ({ text, n: numbers[i] }))
      .filter(({ n }) =>
        side === "old" ? n.new === undefined : n.new !== undefined,
      );
    for (let i = 0; i + size <= sideLines.length; i++) {
      const window = sideLines.slice(i, i + size);
      const lines = window.map((l) => l.text);
      if (hashDiffLines(lines) !== lineHash) continue;
      const startLine = (side === "old" ? window[0].n.old : window[0].n.new)!;
      if (
        !best ||
        Math.abs(startLine - draft.start_line) <
          Math.abs(best.startLine - draft.start_line)
      ) {
        best = { hunkId: hunk.id, lines, startLine };
      }
    }
  }
  return best;
}

/**
 * A stored PR review draft → the diff viewer's comment, following its lines
 * if a PR update moved them unchanged. It is outdated when its lines left the
 * diff or no longer match the hash taken when it was
 * written; it then shows the lines it was written on, kept in `quoted_text`.
 * `lineHash` and `quotedText` describe the current lines, for drafts stored
 * without them.
 */
export function draftToLineComment(
  draft: AgentReviewComment,
  hunks: Map<string, FileHunksData>,
): {
  comment: LineComment;
  lineHash: string | null;
  quotedText: string | null;
  outdated: boolean;
} {
  let found = findCommentedLines(hunks, draft);
  let startLine = draft.start_line;
  // Lines that changed at their old position may have moved unchanged.
  if (
    draft.line_hash &&
    (!found || hashDiffLines(found.lines) !== draft.line_hash)
  ) {
    const moved = findMovedLines(hunks, draft, draft.line_hash);
    if (moved) {
      found = moved;
      ({ startLine } = moved);
    }
  }
  const endLine = startLine + (draft.end_line - draft.start_line);
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
      startLine,
      endLine,
      // An outdated draft shows the lines it was written on, never the
      // lines that replaced them.
      lineContent: outdated
        ? (draft.quoted_text?.split("\n") ?? [])
        : (found?.lines ?? []),
      text,
      createdAt: draft.created_at,
      lineSide: draft.side ?? "new",
    },
    lineHash,
    quotedText: found ? found.lines.join("\n") : null,
    outdated,
  };
}
