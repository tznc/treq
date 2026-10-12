import type { Ref } from "react";
import type { ConflictRegion, GhReviewThread, JjDiffHunk } from "../../lib/api";
import type { Workspace } from "../../lib/api-types";
import type { AgentReviewComment } from "../../lib/api-types-review";
import type { ParsedFileChange } from "../../lib/git-utils";
import {
  FILE_COMMENT_HUNK_ID,
  type LineComment,
  type PendingComment,
} from "../../lib/review";

export { FILE_COMMENT_HUNK_ID };
export type { LineComment, PendingComment };

export interface ChangesDiffViewerProps {
  workspacePath: string;
  repoPath?: string;
  workspaceId?: number;
  /** True when the viewer targets the repository's own working copy. */
  isHomeRepo?: boolean;
  branchName?: string;
  readOnly?: boolean;
  onStagedFilesChange?: (files: string[]) => void;
  onChangedFilesChange?: (files: ParsedFileChange[]) => void;
  onRefreshingChange?: (isRefreshing: boolean) => void;
  initialSelectedFile: string | null;
  onReviewSubmitted?: () => void;
  onCreateAgentWithReview?: (
    reviewMarkdown: string,
    mode: "plan" | "acceptEdits",
    useIdleAgent?: boolean,
  ) => Promise<void>;
  /**
   * Launches a review agent terminal seeded with `prompt`, the same way any
   * other treq terminal agent is launched. `agent` overrides the repo default.
   */
  onStartAgentReview?: (prompt: string, agent?: string) => Promise<void>;
  /**
   * Opens a fresh agent terminal seeded with one local review comment, asking
   * the agent to address it.
   */
  onSendAgentReviewCommentToAgent?: (
    comment: AgentReviewComment,
  ) => Promise<void>;
  conflictedFiles?: string[];
  showCommittedChanges?: boolean;
  onShowCommittedChangesChange?: (show: boolean) => void;
  onMoveFilesToNewWorkspace?: (files: string[]) => void;
  workspace?: Workspace | null;
  baseBranch?: string;
}

export interface ConflictComment {
  id: string;
  conflictId: string;
  filePath: string;
  conflictNumber: number;
  text: string;
  createdAt: string;
}

export interface DiffLineSelection {
  filePath: string;
  lines: Array<{
    hunkIndex: number;
    lineIndex: number;
    content: string;
    isStaged: boolean;
  }>;
}

export interface FileHunksData {
  filePath: string;
  hunks: JjDiffHunk[];
  isLoading: boolean;
  error?: string;
  contentHash?: string;
}

export interface DiffSearchData {
  matches: Array<{
    filePath: string;
    hunkIndex: number;
    lineIndex: number;
    matchIndexInLine: number;
  }>;
  matchesByKey: Map<string, { firstGlobalIndex: number; count: number }>;
}

export interface CommentLineQuery {
  filePath: string;
  hunkId: string;
  lineNumber: number;
  side: "old" | "new";
}

export interface DiffLinePointer {
  filePath: string;
  hunkIndex: number;
  lineIndex: number;
}

export interface LineMouseDownPayload extends DiffLinePointer {
  event: React.MouseEvent;
  lineContent: string;
  isStaged: boolean;
}

export interface CommitInputHandle {
  focus: () => void;
}

export type CommitAction = "commit" | "push" | "pr";

export interface CommitInputProps {
  ref?: Ref<CommitInputHandle>;
  onCommit: (message: string) => void;
  onCommitAndPush: (message: string) => void;
  onCommitAndCreatePR: (message: string) => void;
  disabled: boolean;
  pending: boolean;
  pendingAction?: CommitAction | null;
  canCreatePr?: boolean;
  hasPr?: boolean;
  selectedFileCount?: number;
  totalFileCount?: number;
  workspacePath: string;
}

export interface HunkLinesProps {
  hunk: JjDiffHunk;
  hunkIndex: number;
  filePath: string;
  conflictedFilePaths: Set<string>;
  conflictLineLookups: Map<string, Map<number, ConflictRegion>>;
  firstConflictRegionIdByFile: Map<string, string>;
  expandedContext: Map<string, string[]>;
  conflictComments: Map<string, ConflictComment>;
  openConflictComments: Set<string>;
  editingConflictCommentId: string | null;
  searchData: DiffSearchData;
  debouncedSearchQuery: string;
  currentMatchIndex: number;
  diffLineSelection: DiffLineSelection | null;
  showCommentInput: boolean;
  pendingComment: PendingComment | null;
  editingCommentId: string | null;
  comments: LineComment[];
  conflictFileRefs: React.MutableRefObject<Map<string, HTMLDivElement>>;
  diffFontSize: number;
  handleExpandContext: (
    filePath: string,
    hunkIndex: number,
    direction: "before" | "after",
  ) => void;
  handleLineMouseDown: (payload: LineMouseDownPayload) => void;
  handleLineMouseEnter: (line: DiffLinePointer) => void;
  handleLineMouseUp: () => void;
  handleAddCommentFromSelection: () => void;
  isLineSelected: (line: DiffLinePointer) => boolean;
  saveConflictComment: (args: {
    conflictId: string;
    filePath: string;
    conflictNumber: number;
    text: string;
  }) => void;
  clearConflictComment: (conflictId: string) => void;
  toggleConflictComment: (conflictId: string) => void;
  setOpenConflictComments: React.Dispatch<React.SetStateAction<Set<string>>>;
  startEditConflictComment: (commentId: string) => void;
  cancelEditConflictComment: () => void;
  saveEditConflictComment: (commentId: string, text: string) => void;
  addComment: (text: string) => void;
  cancelComment: () => void;
  deleteComment: (commentId: string) => void;
  startEditComment: (commentId: string) => void;
  cancelEditComment: () => void;
  saveEditComment: (commentId: string, text: string) => void;
  setPendingComment: React.Dispatch<
    React.SetStateAction<PendingComment | null>
  >;
  setShowCommentInput: React.Dispatch<React.SetStateAction<boolean>>;
  getCommentsForLine: (query: CommentLineQuery) => LineComment[];
  getThreadsForLine: (query: CommentLineQuery) => GhReviewThread[];
  collapsedThreadIds: Set<string>;
  toggleThreadCollapse: (threadId: string) => void;
}
