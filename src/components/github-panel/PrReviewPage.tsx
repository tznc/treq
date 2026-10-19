/* eslint-disable max-lines */
import { useEffect, useMemo, useRef, useState } from "react";
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
  createReviewDraft,
  ghCurrentUser,
  ghPrDiff,
  ghReplyPrReviewThread,
  ghSubmitPrReview,
  ghViewPr,
  type GhReviewEvent,
  updateAgentReviewComment,
} from "../../lib/api";
import { AGENT_REVIEW_TARGET_GITHUB_PR } from "../../lib/api-types-review";
import { prepareGithubPrReview } from "../../lib/agent-review-launch";
import {
  commentsToGhReview,
  draftToLineComment,
  parseUnifiedDiff,
} from "../../lib/pr-review";
import { useGpuiScreen } from "../../lib/gpui-screen";
import { cn, getFileName } from "../../lib/utils";
import { pollMs } from "../../lib/swr-cache";
import { useDiffSettingsStore } from "../../stores/diffSettingsStore";
import { useGitRemoteInfo } from "../../hooks/useMergeQueueStatus";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { Textarea } from "../ui/textarea";
import { useToast } from "../ui/toast";
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
  /** GPUI draws the screen over this one (screen/pr_review.rs). */
  gpui?: boolean;
}

interface PrReviewPageProps extends PrReviewScreenProps {
  repoFullName: string;
}

/** Resolves the repo's GitHub remote, then shows {@link PrReviewPage}. */
export function PrReviewScreen(props: PrReviewScreenProps) {
  const { data: remoteInfo } = useGitRemoteInfo(props.repoPath);
  const { addToast } = useToast();
  const { onClose, onStartAgentReview } = props;
  useGpuiScreen(
    props.gpui
      ? { kind: "prReview", repoPath: props.repoPath, prNumber: props.prNumber }
      : null,
    (action) => {
      if (action.kind === "press" && action.id === "prReview.close") onClose();
      if (
        action.kind === "field" &&
        action.field === "prReview.startAgentReview" &&
        onStartAgentReview
      ) {
        const { prompt, agent } = action.value as {
          prompt: string;
          agent?: string | null;
        };
        onStartAgentReview(prompt, agent ?? undefined).catch((error) =>
          addToast({
            title: "Failed to start review",
            description: error instanceof Error ? error.message : String(error),
            type: "error",
          }),
        );
      }
    },
  );
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
  } = useSWR(
    ["gh-pr-diff", repoFullName, prNumber],
    () => ghPrDiff(repoFullName, prNumber),
    // Pick up new pushes so drafts on changed lines turn outdated.
    { refreshInterval: pollMs(60_000) },
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
  const { setComments } = commentState;

  const threads = useGithubReviewThreads({
    repoPath,
    branchName: undefined,
    allFileHunks,
    pr: { owner, repo, number: prNumber },
  });

  // Drafts live in the local DB (target `github_pr`): the user's own
  // (`local-human`) and the AI review's (`local-agent`), which it writes while
  // this screen is open, so poll often. Each draft stores a hash of the lines
  // it covers; when a PR update changes those lines it is shown as outdated.
  const agentReview = useAgentReviewComments({
    repoPath,
    targetType: AGENT_REVIEW_TARGET_GITHUB_PR,
    targetId: String(prNumber),
    refreshMs: 3_000,
  });
  const {
    agentReviewComments: storedDrafts,
    refreshAgentReviewComments: refreshDrafts,
  } = agentReview;
  const drafts = useMemo(
    () =>
      rawDiff === undefined
        ? []
        : storedDrafts
            .filter((row) => row.status !== "resolved")
            .map((row) => ({ row, ...draftToLineComment(row, allFileHunks) })),
    [storedDrafts, rawDiff, allFileHunks],
  );
  useEffect(() => {
    setComments(drafts.map((d) => d.comment));
  }, [drafts, setComments]);

  // The agent's CLI cannot see the diff, so hash its drafts on first sight.
  const hashedIds = useRef<Set<string>>(new Set());
  useEffect(() => {
    const unhashed = drafts.filter(
      (d) => !d.row.line_hash && d.lineHash && !hashedIds.current.has(d.row.id),
    );
    if (unhashed.length === 0) return;
    for (const d of unhashed) hashedIds.current.add(d.row.id);
    void Promise.all(
      unhashed.map((d) =>
        updateAgentReviewComment(repoPath, d.row.id, {
          lineHash: d.lineHash ?? undefined,
        }),
      ),
    ).then(() => refreshDrafts());
  }, [drafts, repoPath, refreshDrafts]);

  const reportDraftError = (error: unknown) =>
    addToast({
      title: "Failed to save draft comment",
      description: error instanceof Error ? error.message : String(error),
      type: "error",
    });

  const addComment = async (text: string) => {
    const pending = commentState.pendingComment;
    if (!text.trim() || !pending) return;
    const { lineHash } = draftToLineComment(
      {
        id: "",
        repo_path: repoPath,
        target_type: AGENT_REVIEW_TARGET_GITHUB_PR,
        target_id: String(prNumber),
        file_path: pending.filePath,
        hunk_id: null,
        start_line: pending.startLine,
        end_line: pending.endLine,
        side: pending.lineSide,
        comment_text: "",
        suggested_replacement: null,
        status: "open",
        source: "local-human",
        created_at: "",
        resolved_at: null,
      },
      allFileHunks,
    );
    try {
      await createReviewDraft(repoPath, {
        targetType: AGENT_REVIEW_TARGET_GITHUB_PR,
        targetId: String(prNumber),
        filePath: pending.filePath,
        startLine: pending.startLine,
        endLine: pending.endLine,
        side: pending.lineSide,
        commentText: text.trim(),
        lineHash,
      });
      commentState.cancelComment();
      await refreshDrafts();
    } catch (error) {
      reportDraftError(error);
    }
  };

  const saveEditComment = async (commentId: string, text: string) => {
    try {
      await updateAgentReviewComment(repoPath, commentId, {
        commentText: text.trim(),
      });
      commentState.cancelEditComment();
      await refreshDrafts();
    } catch (error) {
      reportDraftError(error);
    }
  };

  const deleteComment = (commentId: string) => {
    agentReview.deleteAgentComment(commentId).catch(reportDraftError);
  };

  const postable = drafts.filter((d) => !d.outdated).map((d) => d.comment);
  const outdatedCount = drafts.length - postable.length;

  const { data: viewer } = useSWR("gh-current-user", ghCurrentUser);
  const isOwnPr = Boolean(viewer && pr && viewer === pr.author.login);

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
    const { comments: inline, fileComments } = commentsToGhReview(postable);
    const body = [summary.trim(), fileComments].filter(Boolean).join("\n\n");
    setSubmitting(true);
    try {
      await ghSubmitPrReview(repoFullName, prNumber, event, body, inline);
      // Posted drafts are done; outdated ones stay for the user to handle.
      await Promise.all(
        postable.map((c) => agentReview.resolveAgentComment(c.id)),
      );
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
    event !== "REQUEST_CHANGES" || summary.trim() || postable.length > 0;

  return (
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
                {pr.author.login} wants to merge <code>{pr.head_ref_name}</code>{" "}
                into <code>{pr.base_ref_name}</code> · {files.length} file
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
            </Button>
          )}
          <Popover open={reviewOpen} onOpenChange={setReviewOpen}>
            <PopoverTrigger asChild>
              <Button size="sm" className="gap-2">
                <Send className="w-3 h-3" />
                Finish review
                {postable.length > 0 && ` (${postable.length})`}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" side="bottom" className="w-96">
              <div className="space-y-3">
                <div>
                  <h4 className="font-medium text-sm mb-1">
                    Finish your review
                  </h4>
                  <p className="text-sm text-muted-foreground">
                    {postable.length} pending comment
                    {postable.length === 1 ? "" : "s"} will be posted to GitHub.
                    {outdatedCount > 0 &&
                      ` ${outdatedCount} outdated draft${outdatedCount === 1 ? " stays as a draft" : "s stay as drafts"}.`}
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
                        disabled={isOwnPr && option.value !== "COMMENT"}
                        onChange={() => setEvent(option.value)}
                      />
                      <span>
                        <span className="font-medium">{option.label}</span>
                        <span className="block text-xs text-muted-foreground">
                          {isOwnPr && option.value !== "COMMENT"
                            ? "Pull request authors can't approve or request changes on their own pull request."
                            : option.hint}
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
                addComment={(text) => void addComment(text)}
                saveEditComment={(id, text) => void saveEditComment(id, text)}
                deleteComment={deleteComment}
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
  );
}
