import React, { Fragment } from "react";
import { MessageSquare, Pencil, Plus, X } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../ui/tooltip";
import { cn } from "../../lib/utils";
import { getLanguageFromPath } from "../../lib/syntax-highlight";
import { CommentEditInput } from "../CommentEditInput";
import { CommentInput } from "../CommentInput";
import { InlineConflictCard } from "../InlineConflictCard";
import { AgentReviewInlineList } from "./AgentReviewContext";
import { GithubThreadsInlineList } from "./GithubThreadsInlineList";
import { HighlightedLine } from "../diff/HighlightedLine";
import {
  HunkContextLineRow,
  HunkExpandControl,
  HunkHeaderRow,
} from "./HunkChrome";
import { getQuoteProp } from "./utils";
import {
  computeHunkLineNumbers,
  getLinePrefix,
  getLineTypeClass,
  parseHunkHeader,
} from "../diff/hunk-lines";
import type { HunkLinesProps } from "./types";
import type { ConflictRegion } from "../../lib/api";

export const HunkDiffLine: React.FC<HunkLinesProps & { lineIndex: number }> = (
  props,
) => {
  const {
    hunk,
    hunkIndex,
    filePath,
    conflictedFilePaths,
    searchData,
    debouncedSearchQuery,
    currentMatchIndex,
    diffLineSelection,
    showCommentInput,
    pendingComment,
    editingCommentId,
    handleLineMouseDown,
    handleLineMouseEnter,
    handleLineMouseUp,
    handleAddCommentFromSelection,
    isLineSelected,
    addComment,
    cancelComment,
    deleteComment,
    startEditComment,
    cancelEditComment,
    saveEditComment,
    setPendingComment,
    setShowCommentInput,
    getCommentsForLine,
    getThreadsForLine,
    collapsedThreadIds,
    toggleThreadCollapse,
    lineIndex,
  } = props;
  const line = hunk.lines[lineIndex];
  const lineNumbers = computeHunkLineNumbers(hunk);
  const language = getLanguageFromPath(filePath);
  const isConflictedFile = conflictedFilePaths.has(filePath);
  const lineNum = lineNumbers[lineIndex];
  const actualLineNum = lineNum?.new ?? lineNum?.old ?? lineIndex + 1;
  const currentLineSide: "old" | "new" =
    lineNum?.old !== undefined && lineNum?.new === undefined ? "old" : "new";
  const lineComments = getCommentsForLine({
    filePath,
    hunkId: hunk.id,
    lineNumber: actualLineNum,
    side: currentLineSide,
  });
  const githubThreadsForLine = getThreadsForLine({
    filePath,
    hunkId: hunk.id,
    lineNumber: actualLineNum,
    side: currentLineSide,
  });
  const showCommentInputHere =
    showCommentInput &&
    pendingComment &&
    pendingComment.filePath === filePath &&
    pendingComment.hunkId === hunk.id &&
    lineIndex === pendingComment.displayAtLineIndex;
  const selected = isLineSelected({
    filePath,
    hunkIndex,
    lineIndex,
  });

  const searchKey = `${filePath}:${hunkIndex}:${lineIndex}`;
  const searchLineData = searchData.matchesByKey.get(searchKey);
  let searchCurrentOffset = -1;
  if (searchLineData && debouncedSearchQuery) {
    const globalFirst = searchLineData.firstGlobalIndex;
    if (
      currentMatchIndex >= globalFirst &&
      currentMatchIndex < globalFirst + searchLineData.count
    ) {
      searchCurrentOffset = currentMatchIndex - globalFirst;
    }
  }

  return (
    <Fragment>
      <div
        data-diff-line
        data-search-id={searchKey}
        className={cn(
          "group flex items-stretch",
          getLineTypeClass(line),
          selected && "!bg-blue-500/10",
          selected && "dark:!bg-blue-500/25",
        )}
        onMouseEnter={() =>
          !isConflictedFile &&
          handleLineMouseEnter({
            filePath,
            hunkIndex,
            lineIndex,
          })
        }
        onMouseUp={() => {
          if (!isConflictedFile) handleLineMouseUp();
        }}
        onClick={(event) => {
          event.preventDefault();
        }}
      >
        <div
          data-testid="line-gutter"
          className="w-24 flex-shrink-0 text-muted-foreground select-none border-r border-border/40 flex items-center gap-[4px] cursor-pointer hover:bg-muted/50"
          onMouseDown={(event) =>
            !isConflictedFile &&
            handleLineMouseDown({
              event,
              filePath,
              hunkIndex,
              lineIndex,
              lineContent: line,
              isStaged: false,
            })
          }
        >
          {lineComments.length > 0 && (
            <MessageSquare className="w-3 h-3 text-primary ml-[4px]" />
          )}
          <span className="w-10 text-right text-sm mr-1">
            {lineNum?.old ?? ""}
          </span>
          <span className="w-10 text-right text-sm">{lineNum?.new ?? ""}</span>
        </div>
        <div className="w-6 flex-shrink-0 flex items-center justify-center select-none">
          {!isConflictedFile && (
            <button
              data-comment-button
              className={cn(
                "p-[2px] rounded bg-primary text-primary-foreground hover:bg-primary/90",
                "invisible group-hover:visible",
              )}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.stopPropagation();
                if (diffLineSelection && diffLineSelection.lines.length > 0) {
                  handleAddCommentFromSelection();
                } else {
                  const lineSide: "old" | "new" =
                    lineNum?.old !== undefined && lineNum?.new === undefined
                      ? "old"
                      : "new";
                  setPendingComment({
                    filePath,
                    hunkId: hunk.id,
                    displayAtLineIndex: lineIndex,
                    startLine: actualLineNum,
                    endLine: actualLineNum,
                    lineContent: [line],
                    lineSide,
                  });
                  setShowCommentInput(true);
                }
              }}
              title={
                diffLineSelection && diffLineSelection.lines.length > 1
                  ? "Add comment to selected lines"
                  : "Add comment"
              }
            >
              <Plus className="w-3 h-3" />
            </button>
          )}
        </div>
        <div className="w-5 flex-shrink-0 text-center select-none">
          {getLinePrefix(line)}
        </div>
        <div className="flex-1 px-[8px] py-[2px] whitespace-pre-wrap break-all">
          <HighlightedLine
            content={line.substring(1) || " "}
            language={language}
            searchQuery={debouncedSearchQuery || undefined}
            searchHighlightOffset={searchCurrentOffset}
          />
        </div>
      </div>

      {lineComments.length > 0 && actualLineNum === lineComments[0].endLine && (
        <div className="bg-muted/60 border-y border-border/40 px-[16px] py-[8px] space-y-2">
          {lineComments.map((comment) => {
            const isEditing = editingCommentId === comment.id;

            return (
              <div key={comment.id}>
                {isEditing ? (
                  <div className="bg-background rounded-md p-[12px] border border-border/60">
                    <CommentEditInput
                      initialText={comment.text}
                      onSave={(newText) => saveEditComment(comment.id, newText)}
                      onCancel={cancelEditComment}
                      onDiscard={() => deleteComment(comment.id)}
                    />
                  </div>
                ) : (
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <div
                          data-testid="inline-comment-card"
                          className="group bg-background rounded-md p-[12px] border border-border/60 cursor-pointer hover:shadow-md transition-shadow"
                          onClick={() => startEditComment(comment.id)}
                        >
                          <div className="flex items-start gap-2">
                            <Pencil className="w-3 h-3 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 mt-0.5" />
                            <p className="text-sm whitespace-pre-wrap flex-1">
                              {comment.text}
                            </p>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <button
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    deleteComment(comment.id);
                                  }}
                                  className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground flex-shrink-0"
                                >
                                  <X className="w-3 h-3" />
                                </button>
                              </TooltipTrigger>
                              <TooltipContent>Delete comment</TooltipContent>
                            </Tooltip>
                          </div>
                        </div>
                      </TooltipTrigger>
                      <TooltipContent>Click to edit</TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                )}
              </div>
            );
          })}
        </div>
      )}

      <AgentReviewInlineList
        filePath={filePath}
        lineNumber={actualLineNum}
        lineSide={currentLineSide}
        hunk={hunk}
      />

      <GithubThreadsInlineList
        threads={githubThreadsForLine}
        collapsedThreadIds={collapsedThreadIds}
        toggleThreadCollapse={toggleThreadCollapse}
        filePath={filePath}
        hunkId={hunk.id}
        displayAtLineIndex={lineIndex}
        lineNumber={actualLineNum}
        lineSide={currentLineSide}
        setPendingComment={setPendingComment}
        setShowCommentInput={setShowCommentInput}
      />

      {showCommentInputHere && pendingComment && (
        <CommentInput
          key={`comment-${pendingComment.filePath}-${pendingComment.hunkId}-${pendingComment.displayAtLineIndex}`}
          onSubmit={addComment}
          onCancel={cancelComment}
          filePath={pendingComment.filePath}
          startLine={pendingComment.startLine}
          endLine={pendingComment.endLine}
          quote={getQuoteProp(pendingComment)}
        />
      )}
    </Fragment>
  );
};
HunkDiffLine.displayName = "HunkDiffLine";

export function HunkConflictBlock({
  region,
  filePath,
  firstConflictRegionId,
  ...props
}: Pick<
  HunkLinesProps,
  | "conflictComments"
  | "openConflictComments"
  | "editingConflictCommentId"
  | "saveConflictComment"
  | "clearConflictComment"
  | "toggleConflictComment"
  | "setOpenConflictComments"
  | "startEditConflictComment"
  | "cancelEditConflictComment"
  | "saveEditConflictComment"
  | "searchData"
  | "debouncedSearchQuery"
  | "currentMatchIndex"
  | "conflictFileRefs"
> & {
  region: ConflictRegion;
  filePath: string;
  firstConflictRegionId: string | undefined;
}) {
  const shouldRegisterRef = firstConflictRegionId === region.id;
  return (
    <InlineConflictCard
      region={region}
      conflictComments={props.conflictComments}
      openConflictComments={props.openConflictComments}
      editingConflictCommentId={props.editingConflictCommentId}
      saveConflictComment={props.saveConflictComment}
      clearConflictComment={props.clearConflictComment}
      toggleConflictComment={props.toggleConflictComment}
      setOpenConflictComments={props.setOpenConflictComments}
      startEditConflictComment={props.startEditConflictComment}
      cancelEditConflictComment={props.cancelEditConflictComment}
      saveEditConflictComment={props.saveEditConflictComment}
      searchData={props.searchData}
      debouncedSearchQuery={props.debouncedSearchQuery}
      currentMatchIndex={props.currentMatchIndex}
      className="bg-destructive/10 border-y border-destructive/40 px-[16px] py-[12px] space-y-2"
      registerFileRef={
        shouldRegisterRef
          ? (el) => {
              if (el) {
                props.conflictFileRefs.current.set(filePath, el);
              } else {
                props.conflictFileRefs.current.delete(filePath);
              }
            }
          : undefined
      }
    />
  );
}

const HunkLines: React.FC<HunkLinesProps> = (props) => {
  const {
    hunk,
    hunkIndex,
    filePath,
    conflictLineLookups,
    firstConflictRegionIdByFile,
    expandedContext,
    handleExpandContext,
  } = props;
  const lineNumbers = computeHunkLineNumbers(hunk);
  const conflictLineMap = conflictLineLookups.get(filePath);
  const renderedConflictIds = new Set<string>();
  const firstConflictRegionId = firstConflictRegionIdByFile.get(filePath);
  const beforeKey = `${filePath}:${hunkIndex}:before`;
  const beforeLines = expandedContext.get(beforeKey);
  const { newStart } = parseHunkHeader(hunk.header);
  const hasRoomAbove = newStart > 1 || (beforeLines && beforeLines.length > 0);
  const afterKey = `${filePath}:${hunkIndex}:after`;
  const afterLines = expandedContext.get(afterKey);

  return (
    <Fragment>
      <HunkHeaderRow hunk={hunk} />
      {beforeLines?.map((ctxLine, ctxIdx) => (
        <HunkContextLineRow
          key={`${beforeKey}-${ctxIdx}`}
          filePath={filePath}
          line={ctxLine}
          lineNum={newStart - beforeLines.length + ctxIdx}
        />
      ))}
      {hasRoomAbove && (
        <HunkExpandControl
          direction="before"
          onExpand={() => handleExpandContext(filePath, hunkIndex, "before")}
        />
      )}
      {hunk.lines.map((_line, lineIndex) => {
        const newLineNumber = lineNumbers[lineIndex]?.new;
        if (newLineNumber !== undefined && conflictLineMap) {
          const conflictRegion = conflictLineMap.get(newLineNumber);
          if (conflictRegion) {
            if (
              !renderedConflictIds.has(conflictRegion.id) &&
              conflictRegion.start_line === newLineNumber
            ) {
              renderedConflictIds.add(conflictRegion.id);
              return (
                <HunkConflictBlock
                  key={`conflict-${conflictRegion.id}`}
                  {...props}
                  region={conflictRegion}
                  filePath={filePath}
                  firstConflictRegionId={firstConflictRegionId}
                />
              );
            }
            return null;
          }
        }
        return (
          <HunkDiffLine
            key={`${hunk.id}-line-${lineIndex}`}
            {...props}
            lineIndex={lineIndex}
          />
        );
      })}
      <HunkExpandControl
        direction="after"
        onExpand={() => handleExpandContext(filePath, hunkIndex, "after")}
      />
      {afterLines?.map((ctxLine, ctxIdx) => {
        const { newCount } = parseHunkHeader(hunk.header);
        return (
          <HunkContextLineRow
            key={`${afterKey}-${ctxIdx}`}
            filePath={filePath}
            line={ctxLine}
            lineNum={newStart + newCount + ctxIdx}
          />
        );
      })}
    </Fragment>
  );
};
HunkLines.displayName = "HunkLines";

export { HunkLines };
export type { HunkLinesProps };
