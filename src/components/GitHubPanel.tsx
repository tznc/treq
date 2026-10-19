import {
  AlertCircle,
  Check,
  CircleDot,
  Github,
  GitPullRequest,
  Loader2,
  Plus,
  RefreshCw,
} from "lucide-react";
import { useState } from "react";
import useSWR from "swr";
import { useLocation } from "wouter";
import {
  useDequeueBranches,
  useGitRemoteInfo,
  useMergeQueueEnabled,
} from "../hooks/useMergeQueueStatus";
import { FEATURES } from "../lib/features";
import {
  type GitHubStateFilter,
  githubDetailPath,
  githubPrReviewPath,
  githubListPath,
  githubNewItemPath,
  githubTabPath,
} from "../lib/githubRoutes";
import {
  MERGE_QUEUE_HISTORY_PAGE_SIZE,
  type QueueEntry,
} from "../lib/merge-queue-stacks";
import {
  EMPTY_PR_FILTERS,
  filterPrs,
  type PrFilters,
} from "../lib/github-pr-filters";
import type { GitHubIssueAttachment } from "../lib/promptAttachments";
import { supabase } from "../lib/supabase";
import { pollMs } from "../lib/swr-cache";
import { cn } from "../lib/utils";
import { useAuthStore } from "../stores/authStore";
import { isProSubscription } from "../lib/subscription";
import { CreateIssueForm, IssueDetailPanel } from "./github-panel/IssueDetail";
import { MergeQueueTab } from "./github-panel/MergeQueueTab";
import { NewPrDialog } from "./github-panel/NewPrDialog";
import { PrFilterMenu, PrSearchBar } from "./github-panel/PrFilterBar";
import { PrDetailPanel } from "./github-panel/PrDetail";
import { useGithubRoute } from "./github-panel/useGithubRoute";
import {
  EmptyState,
  ErrorState,
  IssueListItem,
  LoadMoreButton,
  PrListItem,
  StateFilterButtons,
} from "./github-panel/shared";
import {
  useGithubIssuePages,
  useGithubPrPages,
} from "./github-panel/useGithubPagedList";
import { Button } from "./ui/button";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { gpuiDrawsGithub, useGpuiGithubScreen } from "./useGpuiGithubScreen";

interface GitHubPanelProps {
  repoPath: string;
  /** Opens the settings page, where the merge queue opt-in lives. */
  onOpenSettings?: (tab?: string) => void;
  /** Open the agent prompt dialog with a GitHub issue chip pre-attached. */
  onStartPromptFromIssue?: (issue: GitHubIssueAttachment) => void;
  /** Navigate to a workspace after creating/opening one from a PR. */
  onOpenWorkspace?: (workspaceId: number) => void;
  /** GPUI may draw the Issues and Pull Requests tabs (a local repository). */
  gpui?: boolean;
}

const ISSUE_FILTERS: { label: string; value: GitHubStateFilter }[] = [
  { label: "Open", value: "open" },
  { label: "Closed", value: "closed" },
  { label: "All", value: "all" },
];

const PR_FILTERS: { label: string; value: GitHubStateFilter }[] = [
  { label: "Draft", value: "draft" },
  ...ISSUE_FILTERS,
];

export const GitHubPanel: React.FC<GitHubPanelProps> = ({
  repoPath,
  onOpenSettings,
  onStartPromptFromIssue,
  onOpenWorkspace,
  gpui = false,
}) => {
  const { subscription } = useAuthStore();
  const isPro = isProSubscription(subscription);
  const { data: remoteInfo, isLoading: remoteLoading } =
    useGitRemoteInfo(repoPath);
  const { data: queueEnabled } = useMergeQueueEnabled(repoPath);
  const dequeueBranches = useDequeueBranches(repoPath);
  const [showMergedHistory, setShowMergedHistory] = useState(false);
  const [historyLimit, setHistoryLimit] = useState(
    MERGE_QUEUE_HISTORY_PAGE_SIZE,
  );
  const [prFilters, setPrFilters] = useState<PrFilters>(EMPTY_PR_FILTERS);

  const [, navigate] = useLocation();
  const { activeTab, currentFilter, showCreateForm, selectedNumber } =
    useGithubRoute();
  const selectedIssue = activeTab === "issues" ? selectedNumber : null;
  const selectedPr = activeTab === "prs" ? selectedNumber : null;

  const isListTab = activeTab === "issues" || activeTab === "prs";
  // GPUI reads the lists and details itself, so React fetches none of them.
  const gpuiDraws = gpui && isListTab && gpuiDrawsGithub();
  const repoFullName = gpuiDraws ? "" : (remoteInfo?.full_name ?? "");
  useGpuiGithubScreen({
    tab: gpuiDraws ? (activeTab as "issues" | "prs") : null,
    repoPath,
    filter: currentFilter,
    selected: selectedNumber,
    creating: showCreateForm,
    mergeQueue: FEATURES.mergeQueue ? { pro: isPro } : null,
    onStartPromptFromIssue,
    onOpenWorkspace,
  });

  const {
    items: issues,
    error: issuesError,
    isLoading: issuesLoading,
    fetchingNext: issuesFetchingNext,
    hasNextPage: issuesHasNextPage,
    fetchNext: fetchNextIssues,
    refetch: refetchIssues,
  } = useGithubIssuePages(repoFullName, activeTab, currentFilter);
  const {
    items: prs,
    error: prsError,
    isLoading: prsLoading,
    fetchingNext: prsFetchingNext,
    hasNextPage: prsHasNextPage,
    fetchNext: fetchNextPrs,
    refetch: refetchPrs,
  } = useGithubPrPages(repoFullName, activeTab, currentFilter);
  const visiblePrs = filterPrs(prs, prFilters);

  const {
    data: queueEntries = [],
    isLoading: queueLoading,
    mutate: refetchQueue,
  } = useSWR(
    FEATURES.mergeQueue &&
      queueEnabled === true &&
      repoFullName &&
      activeTab === "merge-queue" &&
      isPro
      ? ["repo-branch-queue-statuses-panel", repoFullName]
      : null,
    async () => {
      const { data, error } = await supabase.rpc(
        "get_repo_branch_queue_statuses",
        { p_repo_full_name: repoFullName },
      );
      if (error) throw error;
      return ((data ?? []) as QueueEntry[]).slice().sort((a, b) => {
        if (a.position !== b.position) return a.position - b.position;
        return a.branch_name.localeCompare(b.branch_name);
      });
    },
    { refreshInterval: pollMs(30_000) },
  );

  const isListLoading = activeTab === "issues" ? issuesLoading : prsLoading;

  // A direct link can land here before the remote resolves. Hold the detail
  // panel until the repo name is known so it never asks gh for `--repo ""`.
  const showDetail =
    !!repoFullName &&
    ((activeTab === "issues" && selectedIssue !== null) ||
      (activeTab === "prs" && selectedPr !== null));

  function handleTabChange(v: string) {
    navigate(
      v === "merge-queue"
        ? githubTabPath("merge-queue")
        : githubListPath(v as "issues" | "prs"),
    );
  }

  function handleFilterChange(value: GitHubStateFilter) {
    navigate(githubListPath(activeTab as "issues" | "prs", value));
  }

  function handleSelectIssue(n: number) {
    navigate(githubDetailPath("issues", n, currentFilter));
  }

  function handleSelectPr(n: number) {
    navigate(githubDetailPath("prs", n, currentFilter));
  }

  function handleNewClick() {
    navigate(githubNewItemPath(activeTab as "issues" | "prs", currentFilter));
  }

  function handleCloseDetail() {
    navigate(githubListPath(activeTab as "issues" | "prs", currentFilter), {
      replace: true,
    });
  }

  function handleRefresh() {
    if (activeTab === "issues") void refetchIssues();
    else if (activeTab === "prs") void refetchPrs();
    else void refetchQueue();
  }

  // GPUI draws the tab over the webview; a webview dialog (NewPrDialog)
  // would bring the webview back over it.
  if (gpuiDraws) {
    return (
      <div className="flex h-full bg-background" data-testid="github-panel" />
    );
  }

  return (
    <div className="flex h-full bg-background" data-testid="github-panel">
      {/* List panel */}
      <div
        className={`flex flex-col border-r border-border overflow-hidden ${showDetail ? "w-[380px] shrink-0" : "flex-1"}`}
      >
        <div className="flex items-center px-4 pt-4 pb-2 shrink-0">
          <div className="flex items-center gap-2">
            <Github className="w-5 h-5 text-muted-foreground" />
            <div>
              {remoteLoading ? (
                <h1 className="text-base font-semibold leading-tight text-muted-foreground">
                  <span className="sr-only">GitHub </span>
                  Loading…
                </h1>
              ) : remoteInfo ? (
                <h1 className="text-base font-semibold leading-tight font-mono">
                  <span className="sr-only">GitHub </span>
                  {remoteInfo.full_name}
                </h1>
              ) : (
                <h1 className="text-base font-semibold leading-tight text-destructive">
                  No GitHub remote
                </h1>
              )}
            </div>
          </div>
        </div>

        <div className="px-4 pb-2 shrink-0">
          <Tabs value={activeTab} onValueChange={handleTabChange}>
            <TabsList className="text-base">
              <TabsTrigger value="issues">Issues</TabsTrigger>
              <TabsTrigger value="prs">Pull Requests</TabsTrigger>
              {FEATURES.mergeQueue && (
                <TabsTrigger
                  value="merge-queue"
                  className="inline-flex items-center gap-1.5"
                >
                  Merge Queue
                  {!isPro && (
                    <span className="text-base font-semibold tracking-wide px-1 py-px rounded bg-green-500/20 text-green-700 dark:text-green-400 leading-none">
                      PRO
                    </span>
                  )}
                </TabsTrigger>
              )}
            </TabsList>
          </Tabs>
        </div>

        {isListTab && (
          <div className="flex flex-wrap items-center gap-2 px-4 pb-2 shrink-0">
            <StateFilterButtons
              options={activeTab === "prs" ? PR_FILTERS : ISSUE_FILTERS}
              value={currentFilter}
              onChange={handleFilterChange}
            />
            {activeTab === "prs" && (
              <>
                <PrFilterMenu
                  prs={prs}
                  filters={prFilters}
                  onChange={setPrFilters}
                />
                <PrSearchBar filters={prFilters} onChange={setPrFilters} />
              </>
            )}
            <div className="flex-1" />
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={handleRefresh}
              title="Refresh"
            >
              <RefreshCw className="w-3.5 h-3.5" />
            </Button>
            {remoteInfo && (
              <Button
                size="sm"
                className="h-7 text-base"
                onClick={handleNewClick}
              >
                <Plus className="w-3 h-3 mr-1" />
                New
              </Button>
            )}
          </div>
        )}

        {activeTab === "merge-queue" &&
          isPro &&
          queueEnabled === true &&
          !!remoteInfo && (
            <div
              className="flex items-center gap-2 px-4 pb-2 shrink-0"
              data-testid="merge-queue-toolbar"
            >
              <button
                type="button"
                role="checkbox"
                aria-checked={showMergedHistory}
                aria-label="Merged"
                onClick={() => {
                  const next = !showMergedHistory;
                  setShowMergedHistory(next);
                  if (next) setHistoryLimit(MERGE_QUEUE_HISTORY_PAGE_SIZE);
                }}
                className={cn(
                  "inline-flex items-center gap-1.5 h-7 px-2 rounded-md border text-base transition-colors",
                  showMergedHistory
                    ? "border-primary/40 bg-primary/10 text-foreground"
                    : "border-border bg-background text-muted-foreground hover:text-foreground",
                )}
              >
                <span
                  className={cn(
                    "flex h-3.5 w-3.5 items-center justify-center rounded-sm border",
                    showMergedHistory
                      ? "border-primary bg-primary text-primary-foreground"
                      : "border-muted-foreground/40",
                  )}
                  aria-hidden="true"
                >
                  {showMergedHistory ? <Check className="h-3 w-3" /> : null}
                </span>
                Merged
              </button>
              <div className="flex-1" />
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handleRefresh}
                title="Refresh"
              >
                <RefreshCw className="w-3.5 h-3.5" />
              </Button>
            </div>
          )}

        {showCreateForm && remoteInfo && activeTab === "issues" && (
          <div className="shrink-0 overflow-y-auto">
            <CreateIssueForm
              repoFullName={repoFullName}
              onSuccess={(n) =>
                navigate(githubDetailPath("issues", n, currentFilter), {
                  replace: true,
                })
              }
              onCancel={handleCloseDetail}
            />
          </div>
        )}

        {showCreateForm && remoteInfo && activeTab === "prs" && (
          <NewPrDialog
            repoPath={repoPath}
            repoFullName={repoFullName}
            onSuccess={(n) =>
              navigate(githubDetailPath("prs", n, currentFilter), {
                replace: true,
              })
            }
            onCancel={handleCloseDetail}
          />
        )}

        <div className="flex-1 overflow-y-auto">
          {!remoteInfo && !remoteLoading && activeTab !== "merge-queue" && (
            <div className="flex flex-col items-center justify-center h-full text-center p-8 gap-3">
              <AlertCircle className="w-8 h-8 text-muted-foreground" />
              <p className="text-base text-muted-foreground">
                No GitHub remote detected for this repository.
              </p>
            </div>
          )}

          {remoteInfo && isListTab && isListLoading && (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          )}

          {remoteInfo &&
            !isListLoading &&
            activeTab === "issues" &&
            (issuesError && issues.length === 0 ? (
              <ErrorState
                title="Could not load issues"
                error={issuesError}
                onRetry={() => void refetchIssues()}
              />
            ) : issues.length === 0 ? (
              <EmptyState icon={CircleDot} message="No issues found." />
            ) : (
              <>
                {issues.map((issue) => (
                  <IssueListItem
                    key={issue.number}
                    issue={issue}
                    selected={selectedIssue === issue.number}
                    onClick={() => handleSelectIssue(issue.number)}
                  />
                ))}
                {issuesHasNextPage && (
                  <LoadMoreButton
                    loading={issuesFetchingNext}
                    onClick={() => void fetchNextIssues()}
                  />
                )}
              </>
            ))}

          {remoteInfo &&
            !isListLoading &&
            activeTab === "prs" &&
            (prsError && prs.length === 0 ? (
              <ErrorState
                title="Could not load pull requests"
                error={prsError}
                onRetry={() => void refetchPrs()}
              />
            ) : prs.length === 0 ? (
              <EmptyState
                icon={GitPullRequest}
                message="No pull requests found."
              />
            ) : (
              <>
                {visiblePrs.length === 0 && (
                  <EmptyState
                    icon={GitPullRequest}
                    message="No loaded pull requests match the filters."
                  />
                )}
                {visiblePrs.map((pr) => (
                  <PrListItem
                    key={pr.number}
                    pr={pr}
                    selected={selectedPr === pr.number}
                    onClick={() => handleSelectPr(pr.number)}
                    hideBranches={showDetail}
                  />
                ))}
                {prsHasNextPage && (
                  <LoadMoreButton
                    loading={prsFetchingNext}
                    onClick={() => void fetchNextPrs()}
                  />
                )}
              </>
            ))}

          {activeTab === "merge-queue" && (
            <MergeQueueTab
              isPro={isPro}
              hasRemote={!!remoteInfo}
              repoPath={repoPath}
              queueEnabled={queueEnabled}
              queueLoading={queueLoading}
              queueEntries={queueEntries}
              dequeueBranches={dequeueBranches}
              showMergedHistory={showMergedHistory}
              historyLimit={historyLimit}
              onHistoryLimitChange={setHistoryLimit}
              onOpenSettings={onOpenSettings}
            />
          )}
        </div>
      </div>

      {/* Detail panel */}
      {showDetail && (
        <div className="flex-1 flex flex-col overflow-hidden">
          {activeTab === "issues" && selectedIssue !== null && (
            <IssueDetailPanel
              repoFullName={repoFullName}
              issueNumber={selectedIssue}
              onClose={handleCloseDetail}
              onStartPrompt={onStartPromptFromIssue}
            />
          )}
          {activeTab === "prs" && selectedPr !== null && (
            <PrDetailPanel
              repoPath={repoPath}
              repoFullName={repoFullName}
              prNumber={selectedPr}
              onClose={handleCloseDetail}
              onOpenWorkspace={onOpenWorkspace}
              onReviewChanges={() =>
                navigate(githubPrReviewPath(selectedPr, currentFilter))
              }
            />
          )}
        </div>
      )}
    </div>
  );
};
