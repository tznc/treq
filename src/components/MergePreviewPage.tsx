import { useState } from "react";
import useSWR from "swr";
import {
  type GhMergeMethod,
  type MergeResult,
  type MergeStrategy,
  type Workspace,
  getMergeLandingPlan,
  getMergePreviewDiff,
  ghRepoMergeSettings,
  jjGetCommitsAhead,
  mergeWorkspace,
  mergeWorkspaceOnGithub,
  syncWorkspaceAfterRemoteMerge,
} from "../lib/api";
import {
  invalidatePrStatuses,
  useGitRemoteInfo,
  usePrInfoViaGh,
} from "../hooks/useMergeQueueStatus";
import { MergeLandingPanel } from "./MergeLandingPanel";
import {
  type MergeLocation,
  MergeLocationToggle,
  MergeWarnings,
  githubMergeWarnings,
} from "./MergeGithubControls";
import { MergePreviewChangedFiles } from "./MergePreviewChangedFiles";
import { Button } from "./ui/button";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";
import { Card } from "./ui/card";
import { ArrowLeft, GitPullRequest, Loader2 } from "lucide-react";
import { MergeActionButton } from "./MergeActionButton";
import { MergeCommitsList } from "./MergeCommitsList";
import { useGpuiScreen } from "../lib/gpui-screen";
import { getFullWorkspacePath } from "../lib/utils";

export interface MergePreviewPageProps {
  workspace: Workspace;
  repoPath: string;
  onCancel: () => void;
  onMergeComplete: () => Promise<void>;
  /** GPUI draws the page and merges itself (local workspaces). */
  gpui?: boolean;
}

export const MergePreviewPage = ({
  workspace,
  onCancel,
  onMergeComplete,
  gpui = false,
}: MergePreviewPageProps) => {
  const { addToast } = useToast();
  useGpuiScreen(
    gpui
      ? {
          kind: "mergePreview",
          repoPath: workspace.repo_path,
          workspaceId: workspace.id,
        }
      : null,
    (action) => {
      if (action.kind !== "press") return;
      if (action.id === "mergePreview.cancel") onCancel();
      if (action.id === "mergePreview.merged") void onMergeComplete();
    },
  );

  // State
  const [commitDraft, setCommitDraft] = useState<string | null>(null);
  const [merging, setMerging] = useState(false);
  const [localStrategy, setLocalStrategy] = useState<MergeStrategy>("merge");
  const [githubMethod, setGithubMethod] = useState<GhMergeMethod | null>(null);
  const [location, setLocation] = useState<MergeLocation>("local");
  const [rollupChecked, setRollupChecked] = useState(false);
  const [pushChecked, setPushChecked] = useState(true);

  const fullPath = getFullWorkspacePath(workspace);

  const { data: remoteInfo } = useGitRemoteInfo(workspace.repo_path);
  const { data: prInfo } = usePrInfoViaGh(
    workspace.repo_path,
    workspace.branch_name,
  );
  const openPr =
    remoteInfo && prInfo && prInfo.state === "OPEN" ? prInfo : null;
  const mergedPr =
    remoteInfo && prInfo && prInfo.state === "MERGED" ? prInfo : null;
  const onGithub = location === "github" && openPr !== null;

  const { data: mergeSettings } = useSWR(
    openPr && remoteInfo ? ["gh-merge-settings", remoteInfo.full_name] : null,
    () => ghRepoMergeSettings(remoteInfo!.full_name),
  );

  const { data: plans } = useSWR(
    ["merge-landing-plan", workspace.repo_path, workspace.id],
    async () => {
      const [single, rolledUp] = await Promise.all([
        getMergeLandingPlan(workspace.repo_path, workspace.id, false),
        getMergeLandingPlan(workspace.repo_path, workspace.id, true),
      ]);
      return { single, rolledUp };
    },
  );
  const rollup = !onGithub && rollupChecked && !!plans?.single.can_rollup;
  const plan = plans ? (rollup ? plans.rolledUp : plans.single) : null;

  const mergeStrategy: MergeStrategy = onGithub
    ? (githubMethod ?? mergeSettings?.default_method ?? "merge")
    : localStrategy;
  const setMergeStrategy = (value: MergeStrategy) =>
    onGithub ? setGithubMethod(value) : setLocalStrategy(value);
  const availableMethods: MergeStrategy[] =
    onGithub && mergeSettings
      ? mergeSettings.allowed_methods
      : ["merge", "squash", "rebase"];

  const targetBranch = plan?.target_branch || workspace.target_branch || "main";
  const canPush = !onGithub && !!remoteInfo;
  const push = canPush && pushChecked;

  const {
    data: preview,
    isLoading: loading,
    error: previewError,
  } = useSWR(
    [
      "merge-preview",
      fullPath,
      workspace.repo_path,
      workspace.id,
      targetBranch,
      rollup,
    ],
    async () => {
      const [commits, diffData] = await Promise.all([
        jjGetCommitsAhead(fullPath, targetBranch),
        getMergePreviewDiff(workspace.repo_path, workspace.id, rollup),
      ]);
      return { commits, diffData };
    },
    {
      keepPreviousData: true,
      onError: (error) => {
        addToast({
          title: "Failed to load merge preview",
          description: error instanceof Error ? error.message : String(error),
          type: "error",
        });
      },
    },
  );
  void previewError;
  const commitsAhead = preview?.commits ?? null;
  const diff = preview?.diffData ?? null;

  const defaultCommitMessage = (() => {
    let description = "";
    if (workspace.metadata) {
      try {
        const metadata = JSON.parse(workspace.metadata);
        description = metadata.description || "";
      } catch {
        // If metadata is not valid JSON, description stays empty
      }
    }
    if (mergeStrategy === "squash") return description;
    if (mergeStrategy === "rebase") return "";
    if (description) {
      return `Merge ${workspace.branch_name} into ${targetBranch}\n\n${description}`;
    }
    return `Merge ${workspace.branch_name} into ${targetBranch}`;
  })();
  // An empty GitHub message keeps GitHub's own default.
  const commitMessage = commitDraft ?? (onGithub ? "" : defaultCommitMessage);
  const setCommitMessage = setCommitDraft;

  const reportNotes = (result: MergeResult) => {
    if (result.notes.length === 0) return;
    addToast({
      title: "Stacked pull requests updated",
      description: result.notes.join("\n"),
      type: result.notes.some((note) => note.startsWith("Could not"))
        ? "warning"
        : "success",
    });
  };

  const handleSyncMerged = async () => {
    setMerging(true);
    try {
      const result = await syncWorkspaceAfterRemoteMerge(
        workspace.repo_path,
        workspace.id,
      );
      void invalidatePrStatuses(workspace.repo_path);
      await onMergeComplete();
      addToast({
        title: "Workspace synced",
        description: `Synced ${result.plan.target_branch} after PR #${mergedPr!.number} merged on GitHub`,
        type: "success",
      });
      reportNotes(result);
    } catch (error) {
      addToast({
        title: "Sync failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setMerging(false);
    }
  };

  const handleMerge = async () => {
    if (!onGithub && mergeStrategy !== "rebase" && !commitMessage.trim()) {
      addToast({
        title: "Commit message required",
        type: "error",
      });
      return;
    }

    setMerging(true);
    try {
      let result: MergeResult;
      if (onGithub) {
        const [subject, ...bodyLines] = commitMessage.trim().split("\n");
        result = await mergeWorkspaceOnGithub({
          repoPath: workspace.repo_path,
          workspaceId: workspace.id,
          repoFullName: remoteInfo!.full_name,
          prNumber: openPr!.number,
          method: mergeStrategy,
          subject: subject || undefined,
          body: bodyLines.join("\n").trim() || undefined,
        });
      } else {
        result = await mergeWorkspace(
          workspace.repo_path,
          workspace.id,
          commitMessage,
          mergeStrategy,
          { rollup, push },
        );
      }

      void invalidatePrStatuses(workspace.repo_path);
      await onMergeComplete();

      const actionLabel =
        mergeStrategy === "squash"
          ? "squashed"
          : mergeStrategy === "rebase"
            ? "rebased"
            : "merged";

      addToast({
        title:
          mergeStrategy === "squash"
            ? "Workspace squashed"
            : mergeStrategy === "rebase"
              ? "Workspace rebased"
              : "Workspace merged",
        description: onGithub
          ? `Merged PR #${openPr!.number} on GitHub and synced ${targetBranch}`
          : `Successfully ${actionLabel} ${workspace.branch_name} into ${targetBranch}${push ? " and pushed it" : ""}`,
        type: "success",
      });
      reportNotes(result);
    } catch (error) {
      addToast({
        title: "Merge failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setMerging(false);
    }
  };

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center">
        <Loader2
          className="w-8 h-8 animate-spin text-muted-foreground"
          role="status"
          aria-hidden="true"
        />
      </div>
    );
  }

  const requiresMessage = !onGithub && mergeStrategy !== "rebase";
  const githubWarnings = onGithub
    ? githubMergeWarnings(openPr, targetBranch)
    : [];
  const githubTargetMismatch =
    onGithub && openPr.base_ref_name !== targetBranch;
  const canMerge =
    commitsAhead &&
    commitsAhead.total_count > 0 &&
    !githubTargetMismatch &&
    (!requiresMessage || commitMessage.trim().length > 0);

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header */}
      <div className="border-b p-4 flex items-center gap-4 flex-shrink-0">
        <Button
          variant="ghost"
          size="sm"
          onClick={onCancel}
          aria-label="Go back"
        >
          <ArrowLeft className="w-4 h-4" />
        </Button>
        <div className="flex-1">
          <h1 className="text-lg font-semibold">Merge Preview</h1>
          <p className="text-sm text-muted-foreground">
            {workspace.branch_name} → {targetBranch}
          </p>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-4 space-y-6">
        {/* Commit message and button group - centered card */}
        <div className="flex justify-center px-4">
          <Card className="w-full md:w-3/4 p-6">
            <div className="flex flex-col gap-4">
              {openPr ? (
                <MergeLocationToggle
                  value={location}
                  prNumber={openPr.number}
                  onChange={setLocation}
                />
              ) : null}
              {mergedPr ? (
                <div className="flex flex-wrap items-center gap-3 rounded-md border p-3 text-sm">
                  <GitPullRequest className="h-4 w-4 shrink-0 text-purple-600" />
                  <span className="flex-1 min-w-0">
                    PR #{mergedPr.number} was already merged on GitHub. Sync to
                    update {targetBranch} locally and clean up this workspace.
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={handleSyncMerged}
                    disabled={merging}
                  >
                    Sync merged PR
                  </Button>
                </div>
              ) : null}
              <MergeWarnings warnings={githubWarnings} />
              <div>
                <h2 className="text-sm font-semibold mb-3">
                  {mergeStrategy === "rebase"
                    ? "Commit Message (not used for rebase)"
                    : onGithub
                      ? "Merge Commit Message (optional)"
                      : "Merge Commit Message"}
                </h2>
                <Textarea
                  value={commitMessage}
                  onChange={(e) => setCommitMessage(e.target.value)}
                  placeholder={
                    onGithub
                      ? "Leave empty to use GitHub's default message"
                      : "Enter merge commit message..."
                  }
                  rows={3}
                  maxLength={10000}
                  className="font-mono text-sm"
                  disabled={mergeStrategy === "rebase"}
                />
                {mergeStrategy === "rebase" ? (
                  <p className="text-xs text-muted-foreground mt-2">
                    Rebase keeps the original commit messages.
                  </p>
                ) : null}
              </div>

              {canPush ? (
                <label className="flex items-center gap-2 text-sm">
                  <input
                    id="merge-push-target"
                    type="checkbox"
                    checked={pushChecked}
                    onChange={(e) => setPushChecked(e.target.checked)}
                  />
                  Push {targetBranch} to origin after merging
                </label>
              ) : null}
              <MergeActionButton
                strategy={mergeStrategy}
                availableMethods={availableMethods}
                onStrategyChange={setMergeStrategy}
                onMerge={handleMerge}
                disabled={!canMerge || merging}
                merging={merging}
                onGithub={onGithub}
                commitCount={commitsAhead?.commits.length}
                targetBranch={targetBranch}
              />
            </div>
          </Card>
        </div>

        {plan ? (
          <div className="flex justify-center px-4">
            <div className="w-full md:w-3/4">
              <MergeLandingPanel
                plan={plan}
                pr={onGithub ? openPr : null}
                pushed={push}
                rollup={
                  !onGithub && plans?.single.can_rollup
                    ? {
                        checked: rollupChecked,
                        rootBranch: plans.rolledUp.target_branch,
                        onChange: setRollupChecked,
                      }
                    : null
                }
              />
            </div>
          </div>
        ) : null}

        <MergeCommitsList
          commitsAhead={commitsAhead}
          targetBranch={targetBranch}
        />

        <MergePreviewChangedFiles diff={diff} />
      </div>
    </div>
  );
};
