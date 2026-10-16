import { useState } from "react";
import useSWR from "swr";
import { useMutation } from "../../hooks/useMutation";
import { useRepositoryCacheKey } from "../../lib/active-repository-context";
import { invalidateQueries } from "../../lib/swr-cache";
import {
  FileDiff,
  GitBranch,
  ListChecks,
  Loader2,
  MessageSquare,
  Plus,
  X,
} from "lucide-react";
import { CiStatusButton } from "../CiStatusButton";
import {
  invalidatePrStatuses,
  usePrChecksForPr,
} from "../../hooks/useMergeQueueStatus";
import { useToast } from "../ui/toast";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";
import {
  getWorkspaces,
  ghClosePr,
  ghCreatePrComment,
  ghReopenPr,
  ghSetPrDraft,
  ghViewPr,
  openOrCreateWorkspaceFromPr,
} from "../../lib/api";
import { MarkdownContent } from "../MarkdownContent";
import { PrWorkspaceChecks } from "./PrWorkspaceChecks";
import { compareCiChecksBySeverity } from "../../lib/ci-status";
import {
  CheckEntryRow,
  ErrorState,
  formatDate,
  LabelChip,
  OpenInWebButton,
  StateChip,
  useGhErrorToast,
} from "./shared";

/** Branch glyph (Lucide GitBranch, upright — not the sidebar's Y-flipped form). */
function WorkspaceBranchIcon({ className }: { className?: string }) {
  return <GitBranch className={className} />;
}

/** Branch glyph with a small plus badge at the bottom-right. */
function CreateWorkspaceIcon({ className }: { className?: string }) {
  return (
    <span className={`relative inline-flex ${className ?? ""}`} aria-hidden>
      <WorkspaceBranchIcon className="w-4 h-4" />
      <Plus
        className="absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5"
        strokeWidth={3}
      />
    </span>
  );
}

export function PrDetailPanel({
  repoPath,
  repoFullName,
  prNumber,
  onClose,
  onOpenWorkspace,
  onReviewChanges,
}: {
  repoPath: string;
  repoFullName: string;
  prNumber: number;
  onClose: () => void;
  onOpenWorkspace?: (workspaceId: number) => void;
  /** Opens the full-screen review of this PR's diff. */
  onReviewChanges?: () => void;
}) {
  const { addToast } = useToast();
  const ghErrorToast = useGhErrorToast();
  const [commentBody, setCommentBody] = useState("");

  const {
    data: pr,
    error: prError,
    isLoading,
  } = useSWR(["gh-pr", repoFullName, prNumber], () =>
    ghViewPr(repoFullName, prNumber),
  );

  const repoCacheKey = useRepositoryCacheKey(repoPath);
  const { data: workspaces = [] } = useSWR(
    repoPath ? ["workspaces", repoCacheKey] : null,
    () => getWorkspaces(repoPath),
  );

  const existingWorkspace = pr
    ? (workspaces.find((w) => w.branch_name === pr.head_ref_name) ?? null)
    : null;

  const { data: ciStatus } = usePrChecksForPr(repoFullName, prNumber);

  const openOrCreateWorkspace = useMutation({
    mutationFn: async () => {
      if (!pr) throw new Error("PR not loaded");
      return openOrCreateWorkspaceFromPr(
        repoPath,
        pr.head_ref_name,
        pr.base_ref_name,
        pr.title,
        `From GitHub PR #${pr.number}`,
      );
    },
    onSuccess: async (result) => {
      await invalidateQueries(["workspaces", repoCacheKey]);
      addToast({
        title: result.created ? "Workspace created" : "Workspace opened",
        description: result.created
          ? `Created workspace for ${pr?.head_ref_name ?? "PR head"}`
          : `Opened existing workspace for ${pr?.head_ref_name ?? "PR head"}`,
        type: "success",
      });
      onOpenWorkspace?.(result.workspaceId);
    },
    onError: (error) => {
      addToast({
        title: "Failed to open workspace",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    },
  });

  // The workspace PR badge, View PR button, and Create PR button read the
  // backend PR-status cache, which only the poller refreshes. Re-fetch the
  // head branch so a state change here shows up there right away.
  const refreshWorkspacePrStatus = () => {
    if (pr) void invalidatePrStatuses(repoPath, pr.head_ref_name);
  };

  const addComment = useMutation({
    mutationFn: () => ghCreatePrComment(repoFullName, prNumber, commentBody),
    onSuccess: () => {
      setCommentBody("");
      void invalidateQueries(["gh-pr", repoFullName, prNumber]);
    },
    onError: ghErrorToast("Failed to post comment"),
  });

  const closePr = useMutation({
    mutationFn: () => ghClosePr(repoFullName, prNumber),
    onSuccess: () => {
      void invalidateQueries(["gh-pr", repoFullName, prNumber]);
      void invalidateQueries(["gh-prs", repoFullName]);
      refreshWorkspacePrStatus();
    },
    onError: ghErrorToast("Failed to close pull request"),
  });

  const reopenPr = useMutation({
    mutationFn: () => ghReopenPr(repoFullName, prNumber),
    onSuccess: () => {
      void invalidateQueries(["gh-pr", repoFullName, prNumber]);
      void invalidateQueries(["gh-prs", repoFullName]);
      refreshWorkspacePrStatus();
    },
    onError: ghErrorToast("Failed to reopen pull request"),
  });

  const setDraft = useMutation({
    mutationFn: (draft: boolean) => ghSetPrDraft(repoFullName, prNumber, draft),
    onSuccess: () => {
      void invalidateQueries(["gh-pr", repoFullName, prNumber]);
      void invalidateQueries(["gh-prs", repoFullName]);
      refreshWorkspacePrStatus();
    },
    onError: (error, draft) =>
      ghErrorToast(
        draft
          ? "Failed to convert to draft"
          : "Failed to mark ready for review",
      )(error),
  });

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <span className="text-base font-semibold text-muted-foreground">
          Pull Request #{prNumber}
        </span>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={onClose}
        >
          <X className="w-4 h-4" />
        </Button>
      </div>

      {isLoading && (
        <div className="flex-1 flex items-center justify-center">
          <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
        </div>
      )}

      {prError && !pr && (
        <ErrorState
          title={`Could not load pull request #${prNumber}`}
          error={prError}
        />
      )}

      {pr && (
        <div className="flex-1 overflow-y-auto px-4 py-3 space-y-4">
          <div>
            <div className="flex items-start gap-3">
              <h2 className="text-2xl font-semibold flex-1 min-w-0">
                {pr.title}
              </h2>
              <div className="flex items-center gap-2 shrink-0">
                {onReviewChanges && (
                  <Button
                    size="sm"
                    className="text-base gap-1.5"
                    onClick={onReviewChanges}
                  >
                    <FileDiff className="w-4 h-4" />
                    Review changes
                  </Button>
                )}
                <Button
                  size="sm"
                  variant={
                    existingWorkspace || onReviewChanges ? "outline" : "default"
                  }
                  className="text-base gap-1.5"
                  disabled={openOrCreateWorkspace.isPending}
                  onClick={() => openOrCreateWorkspace.mutate()}
                >
                  {openOrCreateWorkspace.isPending ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : existingWorkspace ? (
                    <WorkspaceBranchIcon className="w-4 h-4" />
                  ) : (
                    <CreateWorkspaceIcon />
                  )}
                  {existingWorkspace ? "Open Workspace" : "Create Workspace"}
                </Button>
                <OpenInWebButton url={pr.url} />
              </div>
            </div>
            <div className="flex items-center gap-2 mt-1 flex-wrap">
              <StateChip state={pr.state} isDraft={Boolean(pr.is_draft)} />
              {ciStatus && ciStatus.total > 0 && (
                <CiStatusButton ciStatus={ciStatus} />
              )}
              <span className="text-base text-muted-foreground font-mono">
                {pr.head_ref_name} → {pr.base_ref_name}
              </span>
            </div>
            <div className="text-base text-muted-foreground mt-1">
              #{pr.number} opened by {pr.author.login} on{" "}
              {formatDate(pr.created_at)}
            </div>
            {pr.labels.length > 0 && (
              <div className="flex gap-1 flex-wrap mt-2">
                {pr.labels.map((l) => (
                  <LabelChip key={l.name} name={l.name} color={l.color} />
                ))}
              </div>
            )}
          </div>

          {ciStatus && ciStatus.total > 0 && (
            <div className="space-y-2">
              <h3 className="text-base font-semibold uppercase tracking-widest text-muted-foreground flex items-center gap-1">
                <ListChecks className="w-3 h-3" />
                Checks ({ciStatus.passed}/{ciStatus.total})
              </h3>
              <div className="border border-border rounded-md divide-y divide-border overflow-hidden">
                {[...ciStatus.checks]
                  .sort(compareCiChecksBySeverity)
                  .map((check) => (
                    <CheckEntryRow key={check.name} check={check} />
                  ))}
              </div>
            </div>
          )}

          {existingWorkspace && (
            <PrWorkspaceChecks
              workspace={existingWorkspace}
              onOpenWorkspace={onOpenWorkspace}
            />
          )}

          {pr.body && (
            <div className="bg-muted/30 rounded-md p-3">
              <MarkdownContent
                content={pr.body}
                className="text-base prose-base prose-code:text-base"
              />
            </div>
          )}

          {(pr.comments ?? []).length > 0 && (
            <div className="space-y-3">
              <h3 className="text-base font-semibold uppercase tracking-widest text-muted-foreground flex items-center gap-1">
                <MessageSquare className="w-3 h-3" />
                Comments ({pr.comments!.length})
              </h3>
              {pr.comments!.map((c) => (
                <div
                  key={c.id}
                  className="bg-muted/30 rounded-md p-3 text-base"
                >
                  <div className="flex items-center gap-1 text-base text-muted-foreground mb-1">
                    <span className="font-medium">{c.author.login}</span>
                    <span>·</span>
                    <span>{formatDate(c.created_at)}</span>
                  </div>
                  <MarkdownContent
                    content={c.body}
                    className="text-base prose-base prose-code:text-base"
                  />
                </div>
              ))}
            </div>
          )}

          <div className="space-y-2">
            <h3 className="text-base font-semibold uppercase tracking-widest text-muted-foreground">
              Add Comment
            </h3>
            <Textarea
              placeholder="Leave a comment..."
              value={commentBody}
              onChange={(e) => setCommentBody(e.target.value)}
              rows={3}
              className="text-base"
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                className="text-base"
                disabled={!commentBody.trim() || addComment.isPending}
                onClick={() => addComment.mutate()}
              >
                {addComment.isPending ? (
                  <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                ) : null}
                Comment
              </Button>
              {pr.state === "OPEN" ? (
                <>
                  {pr.is_draft ? (
                    <Button
                      size="sm"
                      variant="outline"
                      className="text-base"
                      disabled={setDraft.isPending}
                      onClick={() => setDraft.mutate(false)}
                    >
                      {setDraft.isPending ? (
                        <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                      ) : null}
                      Ready for Review
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="text-base"
                      disabled={setDraft.isPending}
                      onClick={() => setDraft.mutate(true)}
                    >
                      {setDraft.isPending ? (
                        <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                      ) : null}
                      Convert to Draft
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="outline"
                    className="text-base"
                    disabled={closePr.isPending}
                    aria-busy={closePr.isPending}
                    onClick={() => closePr.mutate()}
                  >
                    {closePr.isPending ? (
                      <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                    ) : null}
                    {closePr.isPending ? "Closing…" : "Close PR"}
                  </Button>
                </>
              ) : pr.state === "CLOSED" ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="text-base"
                  disabled={reopenPr.isPending}
                  onClick={() => reopenPr.mutate()}
                >
                  {reopenPr.isPending ? (
                    <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                  ) : null}
                  Reopen PR
                </Button>
              ) : null}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
