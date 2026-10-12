import { isBinaryFile, type ParsedFileChange } from "../../lib/git-utils";
import { isDeletedFileStatus } from "../../lib/conflict-deleted-sides";
import {
  FILE_COMMENT_HUNK_ID,
  type FileHunksData,
  type LineComment,
  type PendingComment,
} from "./types";

export type FileBodyKind =
  | "collapsed"
  | "binary"
  | "deleted"
  | "loading"
  | "error"
  | "empty"
  | "large"
  | "hunks";

export interface FileDisplayState {
  additions: number;
  body: FileBodyKind;
  error?: string;
  fileComments: LineComment[];
  fileData: FileHunksData | undefined;
  fileId: string;
  hasFileCommentActivity: boolean;
  isCollapsed: boolean;
  isConflictedFile: boolean;
  isDeleted: boolean;
  isRename: boolean;
  isViewed: boolean;
  showFileCommentInputHere: boolean;
  totalChangedLines: number;
}

function fileSectionDomId(filePath: string): string {
  return `file-section-${filePath.replace(/[^a-zA-Z0-9]/g, "-")}`;
}

export function getFileDisplayState(args: {
  actualConflictedFiles: string[];
  collapsedFiles: Set<string>;
  expandedLargeDiffs: Set<string>;
  file: ParsedFileChange;
  fileHunks: FileHunksData | undefined;
  getFileCommentsForFile: (filePath: string) => LineComment[];
  pendingComment: PendingComment | null;
  showCommentInput: boolean;
  viewedFiles: Map<string, { viewedAt: string; contentHash: string }>;
}): FileDisplayState {
  const filePath = args.file.path;
  const fileData = args.fileHunks;
  const isRename = !!args.file.oldPath;
  const isDeleted =
    isDeletedFileStatus(args.file.workspaceStatus) ||
    isDeletedFileStatus(args.file.stagedStatus);
  const fileComments = args.getFileCommentsForFile(filePath);
  const showFileCommentInputHere =
    args.showCommentInput &&
    args.pendingComment?.filePath === filePath &&
    args.pendingComment.hunkId === FILE_COMMENT_HUNK_ID;
  const hasFileCommentActivity =
    fileComments.length > 0 || showFileCommentInputHere;
  const isCollapsed = hasFileCommentActivity
    ? false
    : isBinaryFile(filePath) || isRename
      ? true
      : args.collapsedFiles.has(filePath);
  const isViewed = args.viewedFiles.has(filePath);
  const isConflictedFile = args.actualConflictedFiles.includes(filePath);

  let additions = 0;
  let deletions = 0;
  if (fileData && !fileData.isLoading && fileData.hunks) {
    for (const hunk of fileData.hunks) {
      for (const line of hunk.lines) {
        if (line.startsWith("+")) additions++;
        else if (line.startsWith("-")) deletions++;
      }
    }
  }
  const totalChangedLines = additions + deletions;

  let body: FileBodyKind = "hunks";
  if (isCollapsed) body = "collapsed";
  else if (isBinaryFile(filePath)) body = "binary";
  else if (isDeleted) body = "deleted";
  else if (!fileData || fileData.isLoading) body = "loading";
  else if (fileData.error) body = "error";
  else if (fileData.hunks.length === 0) body = "empty";
  else if (totalChangedLines > 250 && !args.expandedLargeDiffs.has(filePath)) {
    body = "large";
  }

  return {
    additions,
    body,
    error: fileData?.error,
    fileComments,
    fileData,
    fileId: fileSectionDomId(filePath),
    hasFileCommentActivity,
    isCollapsed,
    isConflictedFile,
    isDeleted,
    isRename,
    isViewed,
    showFileCommentInputHere,
    totalChangedLines,
  };
}
