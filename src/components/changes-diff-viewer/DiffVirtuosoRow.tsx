import React from "react";
import { FileText, X } from "lucide-react";
import { Button } from "../ui/button";
import { Skeleton } from "../ui/skeleton";
import { CommentInput } from "../CommentInput";
import { FileCommentSection } from "./FileCommentSection";
import { FileRowHeader } from "./FileRowHeader";
import { GithubCommentCard } from "./GithubCommentCard";
import {
  contextLineNumber,
  HunkContextLineRow,
  HunkExpandControl,
  HunkHeaderRow,
} from "./HunkChrome";
import { HunkConflictBlock, HunkDiffLine } from "./HunkLines";
import type { DiffVirtuosoItem } from "./buildDiffVirtuosoItems";
import { getFileDisplayState } from "./fileDisplayState";
import { buildQuotedPendingComment, getQuoteProp } from "./utils";
import { cn } from "../../lib/utils";
import { FILE_COMMENT_HUNK_ID, type HunkLinesProps } from "./types";
import type { ParsedFileChange } from "../../lib/git-utils";
import {
  fileCardClass,
  hunkMapFor,
  useDiffRender,
  type DiffRenderContextValue,
} from "./DiffVirtuosoContext";

function FileHeaderItem({
  file,
  isCommitted,
  completeCard,
}: {
  file: ParsedFileChange;
  isCommitted: boolean;
  completeCard: boolean;
}) {
  const ctx = useDiffRender();
  const hunksMap = hunkMapFor({
    isCommitted,
    allFileHunks: ctx.allFileHunks,
    committedFileHunks: ctx.committedFileHunks,
  });
  const display = getFileDisplayState({
    actualConflictedFiles: ctx.actualConflictedFiles,
    collapsedFiles: ctx.collapsedFiles,
    expandedLargeDiffs: ctx.expandedLargeDiffs,
    file,
    fileHunks: hunksMap.get(file.path),
    getFileCommentsForFile: ctx.getFileCommentsForFile,
    pendingComment: ctx.pendingComment,
    showCommentInput: ctx.showCommentInput,
    viewedFiles: ctx.viewedFiles,
  });
  let deletions = 0;
  if (
    display.fileData &&
    !display.fileData.isLoading &&
    display.fileData.hunks
  ) {
    for (const hunk of display.fileData.hunks) {
      for (const line of hunk.lines) {
        if (line.startsWith("-")) deletions++;
      }
    }
  }
  return (
    <div
      id={display.fileId}
      data-file-path={file.path}
      data-testid={`file-row-${file.path}`}
      className={fileCardClass({
        completeCard,
        isStart: true,
        isEnd: completeCard,
      })}
      style={{ fontSize: `${ctx.diffFontSize}px` }}
    >
      <FileRowHeader
        file={file}
        filePath={file.path}
        isCollapsed={display.isCollapsed}
        isRename={display.isRename}
        isViewed={display.isViewed}
        additions={display.additions}
        deletions={deletions}
        readOnly={isCommitted ? true : ctx.readOnly}
        isCommitted={isCommitted}
        fileActionTarget={isCommitted ? null : ctx.fileActionTarget}
        selectedUnstagedFiles={
          isCommitted ? new Set() : ctx.selectedUnstagedFiles
        }
        workspacePath={ctx.workspacePath}
        toggleFileCollapse={ctx.toggleFileCollapse}
        handleMarkFileViewed={ctx.handleMarkFileViewed}
        handleUnmarkFileViewed={ctx.handleUnmarkFileViewed}
        handleDiscardFiles={ctx.handleDiscardFiles}
        addToast={ctx.addToast}
        onAddFileComment={() => {
          if (ctx.collapsedFiles.has(file.path)) {
            ctx.toggleFileCollapse(file.path);
          }
          ctx.setPendingComment({
            filePath: file.path,
            hunkId: FILE_COMMENT_HUNK_ID,
            displayAtLineIndex: -1,
            startLine: 0,
            endLine: 0,
            lineContent: [],
            lineSide: "new",
          });
          ctx.setShowCommentInput(true);
        }}
      />
    </div>
  );
}

function FilePlaceholderItem({
  item,
}: {
  item: Extract<DiffVirtuosoItem, { type: "file-placeholder" }>;
}) {
  const ctx = useDiffRender();
  const inner =
    item.variant === "binary" ? (
      <div className="flex items-center justify-center py-[32px] text-muted-foreground">
        <FileText className="w-5 h-5 mr-[8px] opacity-50" />
        <span>Binary file - no diff available</span>
      </div>
    ) : item.variant === "deleted" ? (
      <div
        data-testid="deleted-file-placeholder"
        className="flex items-center justify-center py-[32px] text-muted-foreground"
      >
        <FileText className="w-5 h-5 mr-[8px] opacity-50" />
        <span>File deleted</span>
      </div>
    ) : item.variant === "loading" ? (
      <div className="flex flex-col gap-[8px] px-[12px] py-[16px]">
        <Skeleton className="h-4 w-3/4" />
        <Skeleton className="h-4 w-1/2" />
        <Skeleton className="h-4 w-5/6" />
      </div>
    ) : item.variant === "error" ? (
      <div className="text-sm text-destructive px-[12px] py-[8px]">
        {item.error}
      </div>
    ) : item.variant === "empty" ? (
      <div className="text-sm text-muted-foreground px-[12px] py-[24px] text-center">
        {item.emptyMessage}
      </div>
    ) : (
      <div className="flex items-center justify-center gap-[12px] h-20 text-muted-foreground">
        <FileText className="w-5 h-5 opacity-50" />
        <span className="text-sm">
          Large diff ({item.largeLineCount} lines)
        </span>
        <Button
          variant="outline"
          size="sm"
          onClick={() => ctx.toggleLargeDiff(item.filePath)}
        >
          View changes
        </Button>
      </div>
    );
  return (
    <div
      className={cn(
        fileCardClass({ completeCard: false, isStart: false, isEnd: false }),
        "font-mono text-sm",
      )}
      onContextMenu={ctx.handleContextMenu}
    >
      {inner}
    </div>
  );
}

function lookupHunk(args: {
  ctx: DiffRenderContextValue;
  filePath: string;
  isCommitted: boolean;
  hunkIndex: number;
}) {
  const map = hunkMapFor({
    isCommitted: args.isCommitted,
    allFileHunks: args.ctx.allFileHunks,
    committedFileHunks: args.ctx.committedFileHunks,
  });
  return map.get(args.filePath)?.hunks[args.hunkIndex];
}

function withHunkProps(args: {
  ctx: DiffRenderContextValue;
  filePath: string;
  isCommitted: boolean;
  hunkIndex: number;
}): HunkLinesProps | null {
  const hunk = lookupHunk(args);
  if (!hunk) return null;
  return {
    ...args.ctx.hunkLinesProps,
    hunk,
    hunkIndex: args.hunkIndex,
    filePath: args.filePath,
  };
}

export function DiffVirtuosoRow({ item }: { item: DiffVirtuosoItem }) {
  const ctx = useDiffRender();
  const bodyWrap = (node: React.ReactNode, isEnd = false) => (
    <div
      className={cn(
        fileCardClass({ completeCard: false, isStart: false, isEnd }),
        "font-mono text-sm",
        isEnd && "mb-4",
      )}
      onContextMenu={ctx.handleContextMenu}
    >
      {node}
    </div>
  );

  switch (item.type) {
    case "file-header":
      return (
        <FileHeaderItem
          file={item.file}
          isCommitted={item.isCommitted}
          completeCard={item.completeCard}
        />
      );
    case "file-placeholder":
      return <FilePlaceholderItem item={item} />;
    case "file-comments": {
      const comments = ctx.getFileCommentsForFile(item.filePath);
      const showInput =
        ctx.showCommentInput &&
        ctx.pendingComment?.filePath === item.filePath &&
        ctx.pendingComment.hunkId === FILE_COMMENT_HUNK_ID;
      return bodyWrap(
        <FileCommentSection
          comments={comments}
          editingCommentId={ctx.editingCommentId}
          showInput={!!showInput}
          onSubmit={ctx.addComment}
          onCancel={ctx.cancelComment}
          onStartEdit={ctx.startEditComment}
          onCancelEdit={ctx.cancelEditComment}
          onSaveEdit={ctx.saveEditComment}
          onDelete={ctx.deleteComment}
        />,
      );
    }
    case "unplaced-threads": {
      const threads = ctx.getUnplacedThreadsForFile(item.filePath);
      return bodyWrap(
        <div
          className="border-b border-sky-500/40 bg-sky-500/5 px-4 py-3 space-y-3"
          data-testid="github-outdated-threads"
        >
          {threads.map((thread) => (
            <GithubCommentCard
              key={thread.id}
              thread={thread}
              collapsed={ctx.collapsedThreadIds.has(thread.id)}
              onToggleCollapse={() => ctx.toggleThreadCollapse(thread.id)}
              onQuote={(quote) => {
                ctx.setPendingComment(
                  buildQuotedPendingComment(
                    {
                      filePath: item.filePath,
                      hunkId: "",
                      displayAtLineIndex: -1,
                      lineNumber: thread.line ?? 0,
                      lineSide: "new",
                    },
                    quote,
                  ),
                );
                ctx.setShowCommentInput(true);
              }}
            />
          ))}
          {ctx.showCommentInput &&
            ctx.pendingComment &&
            ctx.pendingComment.filePath === item.filePath &&
            ctx.pendingComment.hunkId === "" && (
              <CommentInput
                onSubmit={ctx.addComment}
                onCancel={ctx.cancelComment}
                filePath={ctx.pendingComment.filePath}
                startLine={ctx.pendingComment.startLine}
                endLine={ctx.pendingComment.endLine}
                quote={getQuoteProp(ctx.pendingComment)}
              />
            )}
        </div>,
      );
    }
    case "outdated-comments": {
      const outdated = ctx.getOutdatedCommentsForFile(item.filePath);
      return bodyWrap(
        <div className="border-b border-amber-500/40 bg-amber-500/5 px-4 py-3 space-y-3">
          {outdated.map((comment) => (
            <div
              key={comment.id}
              className="bg-background rounded-md p-3 border border-amber-500/30"
            >
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2">
                  <span className="px-1.5 py-0.5 rounded bg-amber-500/25 text-amber-700 dark:text-amber-300 text-xs font-medium">
                    Outdated
                  </span>
                  <span className="text-xs text-muted-foreground">
                    Line{" "}
                    {comment.startLine === comment.endLine
                      ? comment.startLine
                      : `${comment.startLine}-${comment.endLine}`}
                  </span>
                </div>
                <button
                  onClick={() => ctx.deleteComment(comment.id)}
                  className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground"
                  title="Delete"
                >
                  <X className="w-3 h-3" />
                </button>
              </div>
              {comment.lineContent.length > 0 && (
                <pre className="bg-muted/60 rounded px-2 py-1 text-xs mb-2 whitespace-pre-wrap overflow-auto font-mono">
                  {comment.lineContent.join("\n")}
                </pre>
              )}
              <p className="font-sans whitespace-pre-wrap">{comment.text}</p>
            </div>
          ))}
        </div>,
      );
    }
    case "hunk-header": {
      const hunk = lookupHunk({
        ctx,
        filePath: item.filePath,
        isCommitted: item.isCommitted,
        hunkIndex: item.hunkIndex,
      });
      if (!hunk) return null;
      return bodyWrap(
        <HunkHeaderRow
          hunk={hunk}
          onExpandBefore={
            item.canExpandBefore
              ? () =>
                  ctx.handleExpandContext(
                    item.filePath,
                    item.hunkIndex,
                    "before",
                  )
              : undefined
          }
        />,
      );
    }
    case "expand-after":
      return bodyWrap(
        <HunkExpandControl
          direction="after"
          onExpand={() =>
            ctx.handleExpandContext(item.filePath, item.hunkIndex, "after")
          }
        />,
      );
    case "context-line": {
      const hunk = lookupHunk({
        ctx,
        filePath: item.filePath,
        isCommitted: item.isCommitted,
        hunkIndex: item.hunkIndex,
      });
      if (!hunk) return null;
      const ctxKey = `${item.filePath}:${item.hunkIndex}:${item.direction}`;
      const lines = ctx.expandedContext.get(ctxKey) ?? [];
      const line = lines[item.ctxIdx] ?? "";
      return bodyWrap(
        <HunkContextLineRow
          filePath={item.filePath}
          line={line}
          lineNum={contextLineNumber({
            hunk,
            direction: item.direction,
            ctxIdx: item.ctxIdx,
            beforeLength: lines.length,
          })}
        />,
      );
    }
    case "conflict": {
      const props = withHunkProps({
        ctx,
        filePath: item.filePath,
        isCommitted: item.isCommitted,
        hunkIndex: item.hunkIndex,
      });
      const region = Array.from(
        ctx.conflictLineLookups.get(item.filePath)?.values() ?? [],
      ).find((r) => r.id === item.regionId);
      if (!props || !region) return null;
      return bodyWrap(
        <HunkConflictBlock
          {...props}
          region={region}
          filePath={item.filePath}
          firstConflictRegionId={ctx.firstConflictRegionIdByFile.get(
            item.filePath,
          )}
        />,
      );
    }
    case "diff-line": {
      const props = withHunkProps({
        ctx,
        filePath: item.filePath,
        isCommitted: item.isCommitted,
        hunkIndex: item.hunkIndex,
      });
      if (!props) return null;
      return bodyWrap(<HunkDiffLine {...props} lineIndex={item.lineIndex} />);
    }
    case "file-end":
      return (
        <div
          className={cn(
            fileCardClass({ completeCard: false, isStart: false, isEnd: true }),
            "h-0 mb-4",
          )}
        />
      );
    default:
      return null;
  }
}
