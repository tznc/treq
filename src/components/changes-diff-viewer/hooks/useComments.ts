import { useState } from "react";
import { v4 as uuidv4 } from "uuid";
import type { useToast } from "../../ui/toast";
import { resolveFileHunks } from "../utils";
import { computeHunkLineNumbers } from "../../diff/hunk-lines";
import {
  FILE_COMMENT_HUNK_ID,
  type CommentLineQuery,
  type ConflictComment,
  type DiffLineSelection,
  type FileHunksData,
  type LineComment,
  type PendingComment,
} from "../types";

interface UseCommentsParams {
  allFileHunks: Map<string, FileHunksData>;
  committedFileHunks?: Map<string, FileHunksData>;
  diffLineSelection: DiffLineSelection | null;
  clearSelection: () => void;
  setContextMenuPosition: (pos: { x: number; y: number } | null) => void;
  addToast: ReturnType<typeof useToast>["addToast"];
}

export function useComments({
  allFileHunks,
  committedFileHunks,
  diffLineSelection,
  clearSelection,
  setContextMenuPosition,
  addToast,
}: UseCommentsParams) {
  const [comments, setComments] = useState<LineComment[]>([]);
  const [conflictComments, setConflictComments] = useState<
    Map<string, ConflictComment>
  >(new Map());
  const [openConflictComments, setOpenConflictComments] = useState<Set<string>>(
    new Set(),
  );
  const [editingConflictCommentId, setEditingConflictCommentId] = useState<
    string | null
  >(null);
  const [showCommentInput, setShowCommentInput] = useState(false);
  const [pendingComment, setPendingComment] = useState<PendingComment | null>(
    null,
  );
  const [editingCommentId, setEditingCommentId] = useState<string | null>(null);
  const [hasUserAddedComments, setHasUserAddedComments] = useState(false);

  const addComment = (text: string) => {
    if (!text.trim() || !pendingComment) return;
    const newComment: LineComment = {
      id: uuidv4(),
      filePath: pendingComment.filePath,
      hunkId: pendingComment.hunkId,
      startLine: pendingComment.startLine,
      endLine: pendingComment.endLine,
      lineContent: pendingComment.lineContent,
      text: text.trim(),
      createdAt: new Date().toISOString(),
      lineSide: pendingComment.lineSide,
      ...(pendingComment.githubMeta && {
        source: "github",
        githubAuthor: pendingComment.githubMeta.author,
        githubAvatarUrl: pendingComment.githubMeta.avatarUrl,
        githubCommentUrl: pendingComment.githubMeta.commentUrl,
      }),
    };
    setComments((prev) => [...prev, newComment]);
    setHasUserAddedComments(true);
    setShowCommentInput(false);
    setPendingComment(null);
    clearSelection();
  };

  const handleAddCommentFromSelection = () => {
    if (!diffLineSelection || diffLineSelection.lines.length === 0) return;
    const { filePath } = diffLineSelection;
    const working = allFileHunks.get(filePath);
    const committed = committedFileHunks?.get(filePath);
    let fileData = working ?? committed;
    if (working && committed) {
      const [first] = diffLineSelection.lines;
      const workingLine =
        working.hunks[first.hunkIndex]?.lines[first.lineIndex];
      if (workingLine !== first.content) {
        const committedLine =
          committed.hunks[first.hunkIndex]?.lines[first.lineIndex];
        if (committedLine === first.content) fileData = committed;
      }
    }
    if (!fileData) return;
    const lineContents: string[] = [];
    let minLineNum = Infinity;
    let maxLineNum = -Infinity;
    let lastHunkId = "";
    let lastLineIndex = 0;
    let hasOldLines = false;
    let hasNewLines = false;
    for (const line of diffLineSelection.lines) {
      const hunk = fileData.hunks[line.hunkIndex];
      if (!hunk) continue;
      const lineNumbers = computeHunkLineNumbers(hunk);
      const lineNum =
        lineNumbers[line.lineIndex]?.new ??
        lineNumbers[line.lineIndex]?.old ??
        line.lineIndex + 1;
      const lineNumObj = lineNumbers[line.lineIndex];
      minLineNum = Math.min(minLineNum, lineNum);
      maxLineNum = Math.max(maxLineNum, lineNum);
      lineContents.push(line.content);
      lastHunkId = hunk.id;
      lastLineIndex = line.lineIndex;
      if (lineNumObj?.old !== undefined) hasOldLines = true;
      if (lineNumObj?.new !== undefined) hasNewLines = true;
    }
    if (hasOldLines && hasNewLines) {
      setContextMenuPosition(null);
      return;
    }
    const commentLineSide: "old" | "new" = hasOldLines ? "old" : "new";
    setPendingComment({
      filePath,
      hunkId: lastHunkId,
      displayAtLineIndex: lastLineIndex,
      startLine: minLineNum,
      endLine: maxLineNum,
      lineContent: lineContents,
      lineSide: commentLineSide,
    });
    setShowCommentInput(true);
    setContextMenuPosition(null);
  };

  const cancelComment = () => {
    setShowCommentInput(false);
    setPendingComment(null);
  };
  const deleteComment = (commentId: string) => {
    setComments((prev) => prev.filter((c) => c.id !== commentId));
  };
  const startEditComment = (commentId: string) => {
    setEditingCommentId(commentId);
  };
  const cancelEditComment = () => {
    setEditingCommentId(null);
  };
  const saveEditComment = (commentId: string, newText: string) => {
    if (!newText.trim()) return;
    setComments((prev) =>
      prev.map((comment) =>
        comment.id === commentId
          ? { ...comment, text: newText.trim() }
          : comment,
      ),
    );
    setEditingCommentId(null);
  };

  const saveConflictComment = ({
    conflictId,
    filePath,
    conflictNumber,
    text,
  }: {
    conflictId: string;
    filePath: string;
    conflictNumber: number;
    text: string;
  }) => {
    if (!text.trim()) return;
    const comment: ConflictComment = {
      id: uuidv4(),
      conflictId,
      filePath,
      conflictNumber,
      text: text.trim(),
      createdAt: new Date().toISOString(),
    };
    setConflictComments((prev) => {
      const next = new Map(prev);
      next.set(conflictId, comment);
      return next;
    });
  };

  const clearConflictComment = (conflictId: string) => {
    setConflictComments((prev) => {
      const next = new Map(prev);
      next.delete(conflictId);
      return next;
    });
  };

  const startEditConflictComment = (conflictId: string) => {
    setEditingConflictCommentId(conflictId);
  };
  const cancelEditConflictComment = () => {
    setEditingConflictCommentId(null);
  };
  const saveEditConflictComment = (conflictId: string, newText: string) => {
    if (!newText.trim()) return;
    setConflictComments((prev) => {
      const next = new Map(prev);
      const existingComment = prev.get(conflictId);
      if (existingComment)
        next.set(conflictId, { ...existingComment, text: newText.trim() });
      return next;
    });
    setEditingConflictCommentId(null);
  };

  const toggleConflictComment = (conflictId: string) => {
    setOpenConflictComments((prev) => {
      if (prev.has(conflictId)) return new Set();
      return new Set([conflictId]);
    });
  };

  const isCommentOutdated = (comment: LineComment): boolean => {
    if (comment.hunkId === FILE_COMMENT_HUNK_ID) return false;
    const fileData = resolveFileHunks(
      comment.filePath,
      allFileHunks,
      committedFileHunks,
    );
    if (!fileData || fileData.isLoading || !fileData.hunks) return false;
    const hunk = fileData.hunks.find((h) => h.id === comment.hunkId);
    if (!hunk) return true;
    const lineNumbers = computeHunkLineNumbers(hunk);
    const hasMatchingLine = lineNumbers.some((ln) => {
      const actualNum = ln.new ?? ln.old;
      const inRange =
        actualNum &&
        actualNum >= comment.startLine &&
        actualNum <= comment.endLine;
      if (!inRange) return false;
      if (comment.lineSide) {
        const lineSide: "old" | "new" =
          ln.old !== undefined && ln.new === undefined ? "old" : "new";
        return comment.lineSide === lineSide;
      }
      return true;
    });
    return !hasMatchingLine;
  };

  const getOutdatedCommentsForFile = (filePath: string): LineComment[] =>
    comments.filter((c) => c.filePath === filePath && isCommentOutdated(c));

  const getAllOutdatedComments = (): LineComment[] =>
    comments.filter((c) => isCommentOutdated(c));

  const handleCopyOutdatedComments = async () => {
    const outdated = getAllOutdatedComments();
    if (outdated.length === 0) return;
    const markdown = outdated
      .map((c) => {
        const lineRef =
          c.startLine === c.endLine
            ? `${c.filePath}:${c.startLine}`
            : `${c.filePath}:${c.startLine}-${c.endLine}`;
        const codeBlock =
          c.lineContent.length > 0
            ? `\n\`\`\`\n${c.lineContent.join("\n")}\n\`\`\`\n`
            : "";
        return `**${lineRef}**${codeBlock}${c.text}`;
      })
      .join("\n\n");
    try {
      await navigator.clipboard.writeText(markdown);
      addToast({
        title: "Copied",
        description: `${outdated.length} outdated comment${outdated.length > 1 ? "s" : ""} copied to clipboard`,
        type: "success",
      });
    } catch {
      addToast({
        title: "Copy failed",
        description: "Could not copy to clipboard",
        type: "error",
      });
    }
  };

  const getFileCommentsForFile = (filePath: string): LineComment[] =>
    comments.filter(
      (c) => c.filePath === filePath && c.hunkId === FILE_COMMENT_HUNK_ID,
    );

  const getCommentsForLine = ({
    filePath,
    hunkId,
    lineNumber,
    side,
  }: CommentLineQuery) =>
    comments.filter(
      (c) =>
        c.filePath === filePath &&
        c.hunkId === hunkId &&
        lineNumber >= c.startLine &&
        lineNumber <= c.endLine &&
        !isCommentOutdated(c) &&
        (c.lineSide === undefined || c.lineSide === side),
    );

  return {
    comments,
    setComments,
    conflictComments,
    setConflictComments,
    openConflictComments,
    setOpenConflictComments,
    editingConflictCommentId,
    showCommentInput,
    setShowCommentInput,
    pendingComment,
    setPendingComment,
    editingCommentId,
    hasUserAddedComments,
    setHasUserAddedComments,
    addComment,
    handleAddCommentFromSelection,
    cancelComment,
    deleteComment,
    startEditComment,
    cancelEditComment,
    saveEditComment,
    saveConflictComment,
    clearConflictComment,
    startEditConflictComment,
    cancelEditConflictComment,
    saveEditConflictComment,
    toggleConflictComment,
    isCommentOutdated,
    getOutdatedCommentsForFile,
    getFileCommentsForFile,
    getAllOutdatedComments,
    handleCopyOutdatedComments,
    getCommentsForLine,
  };
}
