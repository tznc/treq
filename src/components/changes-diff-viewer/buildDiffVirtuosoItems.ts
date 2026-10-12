import type { ConflictRegion, JjDiffHunk, JjFileChange } from "../../lib/api";
import type { ParsedFileChange } from "../../lib/git-utils";
import { computeHunkLineNumbers, parseHunkHeader } from "../diff/hunk-lines";
import { getFileDisplayState, type FileBodyKind } from "./fileDisplayState";
import type { FileHunksData, LineComment, PendingComment } from "./types";

export type DiffVirtuosoItem =
  | {
      type: "file-header";
      key: string;
      file: ParsedFileChange;
      isCommitted: boolean;
      completeCard: boolean;
    }
  | {
      type: "file-placeholder";
      key: string;
      filePath: string;
      isCommitted: boolean;
      variant: Exclude<FileBodyKind, "collapsed" | "hunks">;
      largeLineCount?: number;
      error?: string;
      emptyMessage: string;
    }
  | {
      type: "file-comments";
      key: string;
      filePath: string;
      isCommitted: boolean;
    }
  | {
      type: "unplaced-threads";
      key: string;
      filePath: string;
      isCommitted: boolean;
    }
  | {
      type: "outdated-comments";
      key: string;
      filePath: string;
      isCommitted: boolean;
    }
  | {
      type: "hunk-header";
      key: string;
      filePath: string;
      isCommitted: boolean;
      hunkIndex: number;
      canExpandBefore: boolean;
    }
  | {
      type: "context-line";
      key: string;
      filePath: string;
      isCommitted: boolean;
      hunkIndex: number;
      direction: "before" | "after";
      ctxIdx: number;
    }
  | {
      type: "conflict";
      key: string;
      filePath: string;
      isCommitted: boolean;
      hunkIndex: number;
      regionId: string;
    }
  | {
      type: "diff-line";
      key: string;
      filePath: string;
      isCommitted: boolean;
      hunkIndex: number;
      lineIndex: number;
    }
  | {
      type: "expand-after";
      key: string;
      filePath: string;
      isCommitted: boolean;
      hunkIndex: number;
    }
  | {
      type: "file-end";
      key: string;
      filePath: string;
      isCommitted: boolean;
    };

export interface DiffVirtuosoIndexMaps {
  filePathToIndex: Map<string, number>;
  searchIdToIndex: Map<string, number>;
}

export interface BuildDiffVirtuosoItemsArgs {
  actualConflictedFiles: string[];
  allFileHunks: Map<string, FileHunksData>;
  collapsedFiles: Set<string>;
  committedFileHunks: Map<string, FileHunksData>;
  committedFiles: ParsedFileChange[];
  conflictLineLookups: Map<string, Map<number, ConflictRegion>>;
  expandedContext: Map<string, string[]>;
  expandedLargeDiffs: Set<string>;
  files: ParsedFileChange[];
  getFileCommentsForFile: (filePath: string) => LineComment[];
  getOutdatedCommentsForFile: (filePath: string) => LineComment[];
  getUnplacedThreadsForFile: (filePath: string) => { id: string }[];
  pendingComment: PendingComment | null;
  showCommentInput: boolean;
  viewedFiles: Map<string, { viewedAt: string; contentHash: string }>;
}

export function committedFileToParsed(file: JjFileChange): ParsedFileChange {
  return {
    path: file.path,
    stagedStatus: "",
    workspaceStatus: file.status,
    isUntracked: false,
  };
}

function indexConflictSearchIds(
  maps: DiffVirtuosoIndexMaps,
  args: {
    filePath: string;
    index: number;
    lineIndex: number;
    region: ConflictRegion;
  },
) {
  maps.searchIdToIndex.set(
    `conflict:${args.filePath}:${args.lineIndex}`,
    args.index,
  );
  const regionLineCount = Math.max(
    args.region.lines?.length ?? 0,
    args.region.content.split("\n").length,
  );
  for (let idx = 0; idx < regionLineCount; idx++) {
    maps.searchIdToIndex.set(`conflict:${args.region.id}:${idx}`, args.index);
  }
}

function pushConflictOrSkipLine(args: {
  conflictLineMap: Map<number, ConflictRegion> | undefined;
  filePath: string;
  hunkIndex: number;
  isCommitted: boolean;
  items: DiffVirtuosoItem[];
  lineIndex: number;
  maps: DiffVirtuosoIndexMaps;
  newLineNumber: number | undefined;
  prefix: string;
  renderedConflictIds: Set<string>;
}): boolean {
  if (args.newLineNumber === undefined || !args.conflictLineMap) return false;
  const conflictRegion = args.conflictLineMap.get(args.newLineNumber);
  if (!conflictRegion) return false;
  if (
    !args.renderedConflictIds.has(conflictRegion.id) &&
    conflictRegion.start_line === args.newLineNumber
  ) {
    args.renderedConflictIds.add(conflictRegion.id);
    const index = args.items.length;
    args.items.push({
      type: "conflict",
      key: `${args.prefix}:conflict:${conflictRegion.id}`,
      filePath: args.filePath,
      isCommitted: args.isCommitted,
      hunkIndex: args.hunkIndex,
      regionId: conflictRegion.id,
    });
    indexConflictSearchIds(args.maps, {
      filePath: args.filePath,
      index,
      lineIndex: args.lineIndex,
      region: conflictRegion,
    });
  }
  return true;
}

function pushHunkItems(
  items: DiffVirtuosoItem[],
  maps: DiffVirtuosoIndexMaps,
  args: {
    conflictLineMap: Map<number, ConflictRegion> | undefined;
    expandedContext: Map<string, string[]>;
    filePath: string;
    hunk: JjDiffHunk;
    hunkIndex: number;
    isCommitted: boolean;
  },
) {
  const { filePath, hunk, hunkIndex, isCommitted, expandedContext } = args;
  const prefix = `${isCommitted ? "c" : "u"}:${filePath}:${hunkIndex}`;
  const beforeKey = `${filePath}:${hunkIndex}:before`;
  const beforeLines = expandedContext.get(beforeKey);
  const { newStart } = parseHunkHeader(hunk.header);

  items.push({
    type: "hunk-header",
    key: `${prefix}:header`,
    filePath,
    isCommitted,
    hunkIndex,
    canExpandBefore: newStart > 1 || Boolean(beforeLines?.length),
  });

  if (beforeLines) {
    for (let ctxIdx = 0; ctxIdx < beforeLines.length; ctxIdx++) {
      items.push({
        type: "context-line",
        key: `${prefix}:before:${ctxIdx}`,
        filePath,
        isCommitted,
        hunkIndex,
        direction: "before",
        ctxIdx,
      });
    }
  }
  const lineNumbers = computeHunkLineNumbers(hunk);
  const renderedConflictIds = new Set<string>();
  for (let lineIndex = 0; lineIndex < hunk.lines.length; lineIndex++) {
    const skippedConflict = pushConflictOrSkipLine({
      conflictLineMap: args.conflictLineMap,
      filePath,
      hunkIndex,
      isCommitted,
      items,
      lineIndex,
      maps,
      newLineNumber: lineNumbers[lineIndex]?.new,
      prefix,
      renderedConflictIds,
    });
    if (skippedConflict) continue;
    const index = items.length;
    items.push({
      type: "diff-line",
      key: `${prefix}:line:${lineIndex}`,
      filePath,
      isCommitted,
      hunkIndex,
      lineIndex,
    });
    maps.searchIdToIndex.set(`${filePath}:${hunkIndex}:${lineIndex}`, index);
  }

  const afterKey = `${filePath}:${hunkIndex}:after`;
  const afterLines = expandedContext.get(afterKey);
  // An empty result means the file has no more lines below this hunk.
  if (afterLines?.length !== 0) {
    items.push({
      type: "expand-after",
      key: `${prefix}:expand-after`,
      filePath,
      isCommitted,
      hunkIndex,
    });
  }
  if (afterLines) {
    for (let ctxIdx = 0; ctxIdx < afterLines.length; ctxIdx++) {
      items.push({
        type: "context-line",
        key: `${prefix}:after:${ctxIdx}`,
        filePath,
        isCommitted,
        hunkIndex,
        direction: "after",
        ctxIdx,
      });
    }
  }
}

function pushFileItems(args: {
  build: BuildDiffVirtuosoItemsArgs;
  file: ParsedFileChange;
  hunksMap: Map<string, FileHunksData>;
  isCommitted: boolean;
  items: DiffVirtuosoItem[];
  maps: DiffVirtuosoIndexMaps;
}) {
  const { file, hunksMap, isCommitted, items, maps, build } = args;
  const filePath = file.path;
  const display = getFileDisplayState({
    actualConflictedFiles: build.actualConflictedFiles,
    collapsedFiles: build.collapsedFiles,
    expandedLargeDiffs: build.expandedLargeDiffs,
    file,
    fileHunks: hunksMap.get(filePath),
    getFileCommentsForFile: build.getFileCommentsForFile,
    pendingComment: build.pendingComment,
    showCommentInput: build.showCommentInput,
    viewedFiles: build.viewedFiles,
  });
  const ns = isCommitted ? "c" : "u";
  maps.filePathToIndex.set(filePath, items.length);
  const completeCard = display.body === "collapsed";
  items.push({
    type: "file-header",
    key: `${ns}:${filePath}:header`,
    file,
    isCommitted,
    completeCard,
  });
  if (completeCard) return;

  if (display.hasFileCommentActivity) {
    items.push({
      type: "file-comments",
      key: `${ns}:${filePath}:file-comments`,
      filePath,
      isCommitted,
    });
  }

  if (display.body !== "hunks" && display.body !== "collapsed") {
    items.push({
      type: "file-placeholder",
      key: `${ns}:${filePath}:placeholder`,
      filePath,
      isCommitted,
      variant: display.body,
      largeLineCount: display.totalChangedLines,
      error: display.error,
      emptyMessage: display.isConflictedFile
        ? "No diff available for this conflicted file (possibly deleted)"
        : "No diff hunks available",
    });
    items.push({
      type: "file-end",
      key: `${ns}:${filePath}:end`,
      filePath,
      isCommitted,
    });
    return;
  }

  const unplaced = build.getUnplacedThreadsForFile(filePath);
  if (unplaced.length > 0) {
    items.push({
      type: "unplaced-threads",
      key: `${ns}:${filePath}:unplaced`,
      filePath,
      isCommitted,
    });
  }
  const outdated = build.getOutdatedCommentsForFile(filePath);
  if (outdated.length > 0) {
    items.push({
      type: "outdated-comments",
      key: `${ns}:${filePath}:outdated`,
      filePath,
      isCommitted,
    });
  }

  const hunks = display.fileData?.hunks ?? [];
  const conflictLineMap = build.conflictLineLookups.get(filePath);
  for (let hunkIndex = 0; hunkIndex < hunks.length; hunkIndex++) {
    pushHunkItems(items, maps, {
      conflictLineMap,
      expandedContext: build.expandedContext,
      filePath,
      hunk: hunks[hunkIndex],
      hunkIndex,
      isCommitted,
    });
  }

  items.push({
    type: "file-end",
    key: `${ns}:${filePath}:end`,
    filePath,
    isCommitted,
  });
}

export function buildDiffVirtuosoItems(args: BuildDiffVirtuosoItemsArgs): {
  items: DiffVirtuosoItem[];
  maps: DiffVirtuosoIndexMaps;
} {
  const items: DiffVirtuosoItem[] = [];
  const maps: DiffVirtuosoIndexMaps = {
    filePathToIndex: new Map(),
    searchIdToIndex: new Map(),
  };
  for (const file of args.files) {
    pushFileItems({
      build: args,
      file,
      hunksMap: args.allFileHunks,
      isCommitted: false,
      items,
      maps,
    });
  }
  for (const file of args.committedFiles) {
    pushFileItems({
      build: args,
      file,
      hunksMap: args.committedFileHunks,
      isCommitted: true,
      items,
      maps,
    });
  }
  return { items, maps };
}
