/* eslint-disable max-lines */
import { useMemo, useRef, useState } from "react";
import useSWR from "swr";
import {
  ArrowLeft,
  Bot,
  ExternalLink,
  FileText,
  GitPullRequest,
  Send,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ghPrDiff,
  ghReplyPrReviewThread,
  ghSubmitPrReview,
  ghViewPr,
  type GhReviewEvent,
} from "../../lib/api";
import {
  AGENT_REVIEW_TARGET_GITHUB_PR,
  type AgentReviewComment,
} from "../../lib/api-types-review";
import { prepareGithubPrReview } from "../../lib/agent-review-launch";
import {
  agentCommentToLineComment,
  commentsToGhReview,
  parseUnifiedDiff,
} from "../../lib/pr-review";
import { cn, getFileName } from "../../lib/utils";
import { useDiffSettingsStore } from "../../stores/diffSettingsStore";
import { useGitRemoteInfo } from "../../hooks/useMergeQueueStatus";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { Textarea } from "../ui/textarea";
import { useToast } from "../ui/toast";
import { AgentReviewContext } from "../changes-diff-viewer/AgentReviewContext";
import { ContextMenu } from "../changes-diff-viewer/ContextMenu";
import { DiffContentArea } from "../changes-diff-viewer/DiffContentArea";
import { GithubThreadReplyContext } from "../changes-diff-viewer/GithubCommentCard";
import { useAgentReviewComments } from "../changes-diff-viewer/hooks/useAgentReviewComments";
import { useComments } from "../changes-diff-viewer/hooks/useComments";
import { useDiffSearch } from "../changes-diff-viewer/hooks/useDiffSearch";
import { useGithubReviewThreads } from "../changes-diff-viewer/hooks/useGithubReviewThreads";
import { useLineSelection } from "../changes-diff-viewer/hooks/useLineSelection";
import type { FileHunksData } from "../changes-diff-viewer/types";

interface PrReviewScreenProps {
  repoPath: string;
  prNumber: number;
  onClose: () => void;
  /** Opens an agent terminal in the home repo seeded with `prompt`. */
  onStartAgentReview?: (prompt: string, agent?: string) => Promise<void>;
}

interface PrReviewPageProps extends PrReviewScreenProps {
  repoFullName: string;
}

/** Resolves the repo's GitHub remote, then shows {@link PrReviewPage}. */
export function PrReviewScreen(props: PrReviewScreenProps) {
  const { data: remoteInfo } = useGitRemoteInfo(props.repoPath);
  if (!remoteInfo?.full_name) {
    return (
      <div className="h-full bg-background" data-testid="pr-review-page" />
    );
  }
  return <PrReviewPage {...props} repoFullName={remoteInfo.full_name} />;
}

const EMPTY_HUNKS = new Map<string, FileHunksData>();
const NO_STRINGS: string[] = [];
const noop = () => {};

const REVIEW_EVENTS: Array<{
  value: GhReviewEvent;
  label: string;
  hint: string;
}> = [
  {
    value: "COMMENT",
    label: "Comment",
    hint: "Submit general feedback without explicit approval.",
  },
  {
    value: "APPROVE",
    label: "Approve",
    hint: "Submit feedback and approve merging these changes.",
  },
  {
    value: "REQUEST_CHANGES",
    label: "Request changes",
    hint: "Submit feedback that must be addressed before merging.",
  },
];

/**
 * Full-screen review of a GitHub PR, laid out like a workspace's Review tab:
 * draft line comments, reply to existing threads, run an AI review, and
 * submit the review to GitHub from a "Finish your review" popover.
 */
function PrReviewPage({
  repoPath,
  repoFullName,
  prNumber,
  onClose,
  onStartAgentReview,
}: PrReviewPageProps) {
  const { addToast } = useToast();
  const { fontSize: diffFontSize } = useDiffSettingsStore();
  const [owner, repo] = repoFullName.split("/");

  const { data: pr } = useSWR(["gh-pr", repoFullName, prNumber], () =>
    ghViewPr(repoFullName, prNumber),
  );
  const {
    data: rawDiff,
    error: diffError,
    isLoading: diffLoading,
  } = useSWR(["gh-pr-diff", repoFullName, prNumber], () =>
    ghPrDiff(repoFullName, prNumber),
  );
  const { files, hunks: allFileHunks } = useMemo(
    () => parseUnifiedDiff(rawDiff ?? ""),
    [rawDiff],
  );

  const diffContainerRef = useRef<HTMLDivElement>(null);
  const diffScrollApiRef = useRef<{
    scrollToFile: (path: string) => void;
    scrollToSearchId: (id: string) => void;
  } | null>(null);
  const conflictFileRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const [collapsedFiles, setCollapsedFiles] = useState<Set<string>>(new Set());
  const [expandedLargeDiffs, setExpandedLargeDiffs] = useState<Set<string>>(
    new Set(),
  );
  const [largeChangesetExpanded, setLargeChangesetExpanded] = useState(false);
  const [viewedFiles, setViewedFiles] = useState<
    Map<string, { viewedAt: string; contentHash: string }>
  >(new Map());
  const [activeFile, setActiveFile] = useState<string | null>(null);

  const toggleIn =
    (setter: React.Dispatch<React.SetStateAction<Set<string>>>) =>
    (path: string) =>
      setter((prev) => {
        const next = new Set(prev);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      });

  const search = useDiffSearch({
    files,
    allFileHunks,
    collapsedFiles,
    expandedLargeDiffs,
    showCommittedChanges: false,
    committedFiles: [],
    committedFileHunks: EMPTY_HUNKS,
    conflictRegionsByFile: new Map(),
    conflictLineLookups: new Map(),
    workspacePath: repoPath,
    diffContainerRef,
    diffScrollApiRef,
  });

  const selection = useLineSelection({
    allFileHunks,
    committedFileHunks: EMPTY_HUNKS,
    onClearFileSelections: noop,
  });

  const commentState = useComments({
    allFileHunks,
    committedFileHunks: EMPTY_HUNKS,
    diffLineSelection: selection.diffLineSelection,
    clearSelection: selection.clearSelection,
    setContextMenuPosition: selection.setContextMenuPosition,
    addToast,
  });
  const { comments, setComments } = commentState;

  const threads = useGithubReviewThreads({
    repoPath,
    branchName: undefined,
    allFileHunks,
    pr: { owner, repo, number: prNumber },
  });

  const agentReview = useAgentReviewComments({
    repoPath,
    targetType: AGENT_REVIEW_TARGET_GITHUB_PR,
    targetId: String(prNumber),
  });

  const addAgentCommentToReview = async (comment: AgentReviewComment) => {
    setComments((prev) => [
      ...prev,
      agentCommentToLineComment(comment, allFileHunks),
    ]);
    await agentReview.resolveAgentComment(comment.id);
  };

  const replyToThread = async (threadId: string, body: string) => {
    await ghReplyPrReviewThread(threadId, body);
    await threads.refreshThreads();
  };

  const [startingAgent, setStartingAgent] = useState(false);
  const handleStartAgentReview = async () => {
    if (!onStartAgentReview) return;
    setStartingAgent(true);
    try {
      const { prompt, agent } = await prepareGithubPrReview({
        repoPath,
        repoFullName,
        prNumber,
        title: pr?.title ?? "",
        fileCount: files.length,
      });
      await onStartAgentReview(prompt, agent);
    } catch (error) {
      addToast({
        title: "Failed to start review",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setStartingAgent(false);
    }
  };

  const [reviewOpen, setReviewOpen] = useState(false);
  const [summary, setSummary] = useState("");
  const [event, setEvent] = useState<GhReviewEvent>("COMMENT");
  const [submitting, setSubmitting] = useState(false);

  const handleSubmitReview = async () => {
    const { comments: inline, fileComments } = commentsToGhReview(comments);
    const body = [summary.trim(), fileComments].filter(Boolean).join("\n\n");
    setSubmitting(true);
    try {
      await ghSubmitPrReview(repoFullName, prNumber, event, body, inline);
      setComments([]);
      setSummary("");
      setEvent("COMMENT");
      setReviewOpen(false);
      await threads.refreshThreads();
      addToast({
        title: "Review submitted",
        description: `#${prNumber}: ${REVIEW_EVENTS.find((e) => e.value === event)?.label}`,
        type: "success",
      });
    } catch (error) {
      addToast({
        title: "Failed to submit review",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setSubmitting(false);
    }
  };

  const canSubmit =
    event !== "REQUEST_CHANGES" || summary.trim() || comments.length > 0;

  return (
    <AgentReviewContext.Provider
      value={{
        getAgentCommentsForLine: agentReview.getAgentCommentsForLine,
        resolveAgentComment: agentReview.resolveAgentComment,
        deleteAgentComment: agentReview.deleteAgentComment,
        applyAgentSuggestion: async () => {},
        addAgentCommentToReview,
        onAgentCommentError: (message) =>
          addToast({
            title: "Review comment action failed",
            description: message,
            type: "error",
          }),
      }}
    >
      <GithubThreadReplyContext.Provider value={replyToThread}>
        <div
          className="flex flex-col h-full min-h-0 bg-background"
          data-testid="pr-review-page"
        >
          <header className="flex items-center gap-3 px-4 py-2 border-b border-border">
            <Button
              size="icon"
              variant="ghost"
              className="h-8 w-8"
              aria-label="Back"
              onClick={onClose}
            >
              <ArrowLeft className="w-4 h-4" />
            </Button>
            <GitPullRequest className="w-4 h-4 text-emerald-500 flex-shrink-0" />
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium truncate">
                {pr?.title ?? `Pull request #${prNumber}`}{" "}
                <span className="text-muted-foreground">#{prNumber}</span>
              </div>
              {pr && (
                <div className="text-xs text-muted-foreground truncate">
                  {pr.author.login} wants to merge{" "}
                  <code>{pr.head_ref_name}</code> into{" "}
                  <code>{pr.base_ref_name}</code> · {files.length} file
                  {files.length === 1 ? "" : "s"}
                </div>
              )}
            </div>
            {pr && (
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                aria-label="Open on GitHub"
                onClick={() => void openUrl(pr.url)}
              >
                <ExternalLink className="w-4 h-4" />
              </Button>
            )}
            {onStartAgentReview && (
              <Button
                size="sm"
                variant="outline"
                className="gap-2"
                disabled={startingAgent || files.length === 0}
                onClick={() => void handleStartAgentReview()}
              >
                <Bot className="w-3 h-3" />
                AI review
                {agentReview.openAgentReviewComments.length > 0 &&
                  ` (${agentReview.openAgentReviewComments.length})`}
              </Button>
            )}
            <Popover open={reviewOpen} onOpenChange={setReviewOpen}>
              <PopoverTrigger asChild>
                <Button size="sm" className="gap-2">
                  <Send className="w-3 h-3" />
                  Finish review
                  {comments.length > 0 && ` (${comments.length})`}
                </Button>
              </PopoverTrigger>
              <PopoverContent align="end" side="bottom" className="w-96">
                <div className="space-y-3">
                  <div>
                    <h4 className="font-medium text-sm mb-1">
                      Finish your review
                    </h4>
                    <p className="text-sm text-muted-foreground">
                      {comments.length} pending comment
                      {comments.length === 1 ? "" : "s"} will be posted to
                      GitHub.
                    </p>
                  </div>
                  <Textarea
                    value={summary}
                    onChange={(e) => setSummary(e.target.value)}
                    placeholder="Leave a comment"
                    className="min-h-[80px] text-sm"
                  />
                  <div role="radiogroup" className="space-y-2">
                    {REVIEW_EVENTS.map((option) => (
                      <label
                        key={option.value}
                        aria-label={option.label}
                        className="flex items-start gap-2 text-sm cursor-pointer"
                      >
                        <input
                          type="radio"
                          name="pr-review-event"
                          className="mt-1"
                          checked={event === option.value}
                          onChange={() => setEvent(option.value)}
                        />
                        <span>
                          <span className="font-medium">{option.label}</span>
                          <span className="block text-xs text-muted-foreground">
                            {option.hint}
                          </span>
                        </span>
                      </label>
                    ))}
                  </div>
                  <div className="flex justify-end">
                    <Button
                      size="sm"
                      disabled={submitting || !canSubmit}
                      onClick={() => void handleSubmitReview()}
                    >
                      Submit review
                    </Button>
                  </div>
                </div>
              </PopoverContent>
            </Popover>
          </header>

          <div
            className="flex flex-1 min-h-0 overflow-hidden"
            onClick={selection.handleBackgroundClick}
          >
            <nav
              aria-label="Changed files"
              className="w-64 flex-shrink-0 border-r border-border overflow-y-auto py-2"
            >
              {files.map((file) => (
                <button
                  key={file.path}
                  title={file.path}
                  onClick={() => {
                    setActiveFile(file.path);
                    diffScrollApiRef.current?.scrollToFile(file.path);
                  }}
                  className={cn(
                    "w-full flex items-center gap-2 px-3 py-1 text-left text-xs hover:bg-accent",
                    activeFile === file.path && "bg-accent",
                  )}
                >
                  <FileText className="w-3 h-3 flex-shrink-0 text-muted-foreground" />
                  <span className="truncate flex-1">
                    {getFileName(file.path)}
                  </span>
                  <span className="text-[10px] text-muted-foreground">
                    {file.workspaceStatus}
                  </span>
                </button>
              ))}
            </nav>
            <div className="flex-1 flex flex-col min-w-0">
              {diffError ? (
                <div className="p-6 text-sm text-destructive">
                  Failed to load the PR diff: {String(diffError)}
                </div>
              ) : (
                <DiffContentArea
                  {...search}
                  initialLoading={diffLoading}
                  loadingAllHunks={false}
                  files={files}
                  allFileHunks={allFileHunks}
                  committedFiles={[]}
                  committedFileHunks={EMPTY_HUNKS}
                  showCommittedChanges={false}
                  largeChangesetExpanded={largeChangesetExpanded}
                  setLargeChangesetExpanded={setLargeChangesetExpanded}
                  actualConflictedFiles={NO_STRINGS}
                  conflictLineLookups={new Map()}
                  firstConflictRegionIdByFile={new Map()}
                  expandedContext={new Map()}
                  {...commentState}
                  conflictFileRefs={conflictFileRefs}
                  diffFontSize={diffFontSize}
                  handleExpandContext={noop}
                  diffLineSelection={selection.diffLineSelection}
                  handleLineMouseDown={selection.handleLineMouseDown}
                  handleLineMouseEnter={selection.handleLineMouseEnter}
                  handleLineMouseUp={selection.handleLineMouseUp}
                  isLineSelected={selection.isLineSelected}
                  getThreadsForLine={threads.getThreadsForLine}
                  getUnplacedThreadsForFile={threads.getUnplacedThreadsForFile}
                  collapsedThreadIds={threads.collapsedThreadIds}
                  toggleThreadCollapse={threads.toggleThreadCollapse}
                  collapsedFiles={collapsedFiles}
                  viewedFiles={viewedFiles}
                  expandedLargeDiffs={expandedLargeDiffs}
                  readOnly
                  fileActionTarget={null}
                  selectedUnstagedFiles={new Set()}
                  workspacePath={repoPath}
                  toggleFileCollapse={toggleIn(setCollapsedFiles)}
                  toggleLargeDiff={toggleIn(setExpandedLargeDiffs)}
                  handleMarkFileViewed={(path) => {
                    setViewedFiles((prev) =>
                      new Map(prev).set(path, {
                        viewedAt: new Date().toISOString(),
                        contentHash: "",
                      }),
                    );
                    setCollapsedFiles((prev) => new Set(prev).add(path));
                  }}
                  handleUnmarkFileViewed={(path) =>
                    setViewedFiles((prev) => {
                      const next = new Map(prev);
                      next.delete(path);
                      return next;
                    })
                  }
                  handleDiscardFiles={noop}
                  handleContextMenu={selection.handleContextMenu}
                  addToast={addToast}
                  diffContainerRef={diffContainerRef}
                  diffScrollApiRef={diffScrollApiRef}
                />
              )}
            </div>
          </div>

          {selection.contextMenuPosition &&
            selection.diffLineSelection &&
            selection.diffLineSelection.lines.length > 0 && (
              <ContextMenu
                position={selection.contextMenuPosition}
                onAddComment={commentState.handleAddCommentFromSelection}
                onCopyLocation={noop}
                onCopyLines={noop}
              />
            )}
        </div>
      </GithubThreadReplyContext.Provider>
    </AgentReviewContext.Provider>
  );
}
