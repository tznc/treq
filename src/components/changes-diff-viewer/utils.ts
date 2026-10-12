import type { JjDiffHunk } from "../../lib/api";
import type { ParsedFileChange } from "../../lib/git-utils";
import { toApiLineComment, toLocalLineComment } from "../../lib/review";
import { computeHunkLineNumbers } from "../diff/hunk-lines";
import type { FileHunksData, PendingComment } from "./types";
import type { GithubQuote } from "./GithubCommentCard";

export { toApiLineComment, toLocalLineComment };

/**
 * Resolve hunks for a file path, preferring the working-tree map and falling
 * back to committed Review-tab hunks. Committed-only files live exclusively in
 * `committedFileHunks`, so selection/comment helpers must consult both.
 */
export function resolveFileHunks(
  filePath: string,
  allFileHunks: Map<string, FileHunksData>,
  committedFileHunks?: Map<string, FileHunksData>,
): FileHunksData | undefined {
  return allFileHunks.get(filePath) ?? committedFileHunks?.get(filePath);
}

export function buildQuotedPendingComment(
  args: {
    filePath: string;
    hunkId: string;
    displayAtLineIndex: number;
    lineNumber: number;
    lineSide: "old" | "new";
  },
  quote: GithubQuote,
): PendingComment {
  return {
    filePath: args.filePath,
    hunkId: args.hunkId,
    displayAtLineIndex: args.displayAtLineIndex,
    startLine: args.lineNumber,
    endLine: args.lineNumber,
    lineContent: [quote.text],
    lineSide: args.lineSide,
    githubMeta: {
      author: quote.author,
      avatarUrl: quote.avatarUrl,
      commentUrl: quote.commentUrl,
    },
  };
}

export function getQuoteProp(
  pendingComment: PendingComment | null,
): { text: string; author?: string } | undefined {
  if (!pendingComment?.githubMeta) return undefined;
  return {
    text: pendingComment.lineContent.join(" "),
    author: pendingComment.githubMeta.author,
  };
}

export const filesEqual = (
  filesA: ParsedFileChange[],
  filesB: ParsedFileChange[],
): boolean => {
  if (filesA.length !== filesB.length) return false;
  for (let idx = 0; idx < filesA.length; idx++) {
    if (
      filesA[idx].path !== filesB[idx].path ||
      filesA[idx].stagedStatus !== filesB[idx].stagedStatus ||
      filesA[idx].workspaceStatus !== filesB[idx].workspaceStatus ||
      filesA[idx].isUntracked !== filesB[idx].isUntracked
    ) {
      return false;
    }
  }
  return true;
};

/**
 * Committed Show toggle: only committed-only (no WC changes, not conflicted)
 * files may hide. Dirty or conflicted committed paths stay visible.
 */
export function shouldShowCommittedFile(
  path: string,
  showCommittedChanges: boolean,
  alwaysVisiblePaths: ReadonlySet<string>,
): boolean {
  return showCommittedChanges || alwaysVisiblePaths.has(path);
}

export function filterVisibleCommittedFiles<T extends { path: string }>(
  committedFiles: T[],
  showCommittedChanges: boolean,
  alwaysVisiblePaths: ReadonlySet<string>,
): T[] {
  if (showCommittedChanges) return committedFiles;
  return committedFiles.filter((file) =>
    shouldShowCommittedFile(file.path, false, alwaysVisiblePaths),
  );
}

/**
 * Source text of every line in a hunk, keyed `"<side>:<lineNumber>"` and with
 * the diff marker stripped. Used to diff a suggested replacement against the
 * lines it replaces.
 */
export const buildHunkLineTexts = (hunk: JjDiffHunk): Map<string, string> => {
  const lineNumbers = computeHunkLineNumbers(hunk);
  const texts = new Map<string, string>();
  hunk.lines.forEach((line, index) => {
    const content = line.substring(1);
    const numbers = lineNumbers[index];
    if (numbers?.new !== undefined) texts.set(`new:${numbers.new}`, content);
    if (numbers?.old !== undefined) texts.set(`old:${numbers.old}`, content);
  });
  return texts;
};

export const computeHunksHash = (hunks: JjDiffHunk[]): string => {
  const content = hunks
    .map((hunk) => hunk.header + hunk.lines.join(""))
    .join("|");
  let hash = 5381;
  for (let idx = 0; idx < content.length; idx++) {
    hash = (hash * 33 + content.charCodeAt(idx)) % 4294967296;
  }
  return hash.toString(16);
};
