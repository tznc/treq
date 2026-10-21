import { useEffect, useState } from "react";
import useSWR from "swr";
import {
  type MergeStrategy,
  type Workspace,
  getWorkspaceDiff,
  jjGetCommitsAhead,
  mergeWorkspace,
} from "../lib/api";
import { Button } from "./ui/button";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";
import { Card } from "./ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  FileText,
  GitBranch,
  GitCommitHorizontal,
  GitMerge,
  Loader2,
} from "lucide-react";
import { useGpuiScreen } from "../lib/gpui-screen";
import { cn, getFullWorkspacePath } from "../lib/utils";

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
  const [expandedFiles, setExpandedFiles] = useState<Set<string> | null>(null);
  const [mergeStrategy, setMergeStrategy] = useState<MergeStrategy>("merge");

  const targetBranch = workspace.target_branch || "main";
  const fullPath = getFullWorkspacePath(workspace);

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
    ],
    async () => {
      const [commits, diffData] = await Promise.all([
        jjGetCommitsAhead(fullPath, targetBranch),
        getWorkspaceDiff(workspace.repo_path, workspace.id),
      ]);
      return { commits, diffData };
    },
    {
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

  useEffect(() => {
    if (!diff?.hunks_by_file) return;
    setExpandedFiles(new Set(diff.hunks_by_file.map((file) => file.path)));
  }, [diff]);

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
  const commitMessage = commitDraft ?? defaultCommitMessage;
  const setCommitMessage = setCommitDraft;
  const expandedFileSet = expandedFiles ?? new Set<string>();

  // Handle merge
  const handleMerge = async () => {
    if (mergeStrategy !== "rebase" && !commitMessage.trim()) {
      addToast({
        title: "Commit message required",
        type: "error",
      });
      return;
    }

    setMerging(true);
    try {
      // Use new high-level API with merge strategy
      await mergeWorkspace(
        workspace.repo_path,
        workspace.id,
        commitMessage,
        mergeStrategy,
      );

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
        description: `Successfully ${actionLabel} ${workspace.branch_name} into ${targetBranch}`,
        type: "success",
      });
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

  // Toggle file expansion
  const toggleFile = (path: string) => {
    setExpandedFiles((prev) => {
      const next = new Set(prev ?? []);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
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

  const requiresMessage = mergeStrategy !== "rebase";
  const canMerge =
    commitsAhead &&
    commitsAhead.total_count > 0 &&
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
              <div>
                <h2 className="text-sm font-semibold mb-3">
                  {mergeStrategy === "rebase"
                    ? "Commit Message (not used for rebase)"
                    : "Merge Commit Message"}
                </h2>
                <Textarea
                  value={commitMessage}
                  onChange={(e) => setCommitMessage(e.target.value)}
                  placeholder="Enter merge commit message..."
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

              {/* Button group - aligned to left */}
              <div className="flex justify-start">
                <div className="inline-flex items-center rounded-md overflow-hidden">
                  <Button
                    onClick={handleMerge}
                    disabled={!canMerge || merging}
                    className="gap-2 rounded-none"
                    style={{
                      backgroundColor: "hsl(142, 76%, 36%)",
                    }}
                  >
                    {merging ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <GitMerge className="w-4 h-4" />
                    )}
                    {merging
                      ? "Merging..."
                      : mergeStrategy === "squash"
                        ? "Squash and merge"
                        : mergeStrategy === "rebase"
                          ? "Rebase and merge"
                          : "Confirm merge"}
                  </Button>
                  <div
                    className="w-px opacity-30"
                    style={{ backgroundColor: "currentColor" }}
                  />
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        disabled={!canMerge || merging}
                        className="rounded-none px-2"
                        style={{
                          backgroundColor: "hsl(142, 76%, 36%)",
                        }}
                      >
                        <ChevronDown className="h-4 w-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="w-64">
                      <DropdownMenuRadioGroup
                        value={mergeStrategy}
                        onValueChange={(value) =>
                          setMergeStrategy(value as MergeStrategy)
                        }
                      >
                        <DropdownMenuRadioItem
                          value="merge"
                          className="flex-col items-start py-3"
                        >
                          <div className="font-semibold flex items-center gap-2">
                            <GitMerge className="h-4 w-4" />
                            Create a merge commit
                          </div>
                          <div className="text-xs text-muted-foreground mt-1 pl-6">
                            All commits from this branch will be added to the
                            base branch via a merge commit
                          </div>
                        </DropdownMenuRadioItem>
                        <DropdownMenuRadioItem
                          value="squash"
                          className="flex-col items-start py-3"
                        >
                          <div className="font-semibold flex items-center gap-2">
                            <GitCommitHorizontal className="h-4 w-4" />
                            Squash and merge
                          </div>
                          <div className="text-xs text-muted-foreground mt-1 pl-6">
                            The {commitsAhead?.commits.length || "multiple"}{" "}
                            commits from this branch will be combined into one
                            commit
                          </div>
                        </DropdownMenuRadioItem>
                        <DropdownMenuRadioItem
                          value="rebase"
                          className="flex-col items-start py-3"
                        >
                          <div className="font-semibold flex items-center gap-2">
                            <GitBranch className="h-4 w-4" />
                            Rebase and merge
                          </div>
                          <div className="text-xs text-muted-foreground mt-1 pl-6">
                            Commits will be rebased onto {targetBranch} with a
                            linear history
                          </div>
                        </DropdownMenuRadioItem>
                      </DropdownMenuRadioGroup>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              </div>
            </div>
          </Card>
        </div>

        {/* Commits to be merged */}
        <section>
          <h2 className="text-sm font-semibold mb-3">
            Commits to be merged ({commitsAhead?.total_count || 0})
          </h2>
          {commitsAhead && commitsAhead.commits.length > 0 ? (
            <div className="border rounded-lg divide-y">
              {commitsAhead.commits.map((commit) => (
                <div key={commit.short_id} className="p-3">
                  <div className="flex items-start gap-3">
                    <code className="text-xs bg-muted px-1.5 py-0.5 rounded font-mono">
                      {commit.short_id}
                    </code>
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium truncate">
                        {commit.description || "(no description)"}
                      </p>
                      <div className="flex items-center gap-2 mt-0.5 flex-wrap">
                        <span className="text-xs text-muted-foreground">
                          {commit.author_name}
                        </span>
                        {commit.insertions > 0 || commit.deletions > 0 ? (
                          <span className="text-xs text-muted-foreground">
                            <span className="text-green-600">
                              +{commit.insertions}
                            </span>{" "}
                            <span className="text-red-600">
                              -{commit.deletions}
                            </span>
                          </span>
                        ) : null}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="border rounded-lg p-6 text-center text-muted-foreground">
              No commits ahead of {targetBranch}
            </div>
          )}
        </section>

        {/* Combined diff */}
        <section>
          <h2 className="text-sm font-semibold mb-3">
            Changed Files ({diff?.committed_files.length || 0})
          </h2>
          {diff && diff.committed_files.length > 0 ? (
            <div className="border rounded-lg divide-y">
              {diff.hunks_by_file.map((fileDiff) => {
                const isExpanded = expandedFileSet.has(fileDiff.path);
                return (
                  <div key={fileDiff.path}>
                    <button
                      className="w-full p-3 flex items-center gap-2 text-left hover:bg-muted/50 transition-colors"
                      onClick={() => toggleFile(fileDiff.path)}
                      type="button"
                    >
                      {isExpanded ? (
                        <ChevronDown className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                      ) : (
                        <ChevronRight className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                      )}
                      <FileText className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                      <span className="font-mono text-sm flex-1 truncate">
                        {fileDiff.path}
                      </span>
                    </button>
                    {isExpanded && fileDiff.hunks.length > 0 && (
                      <div className="border-t bg-muted/30">
                        {fileDiff.hunks.map((hunk, hunkIndex) => (
                          <pre
                            key={hunk.id || hunkIndex}
                            className="p-2 text-xs font-mono overflow-x-auto"
                          >
                            <div className="text-muted-foreground mb-1">
                              {hunk.header}
                            </div>
                            {hunk.lines.map((line, lineIndex) => (
                              <div
                                key={lineIndex}
                                className={cn(
                                  line.startsWith("+") &&
                                    !line.startsWith("+++") &&
                                    "bg-emerald-500/20",
                                  line.startsWith("-") &&
                                    !line.startsWith("---") &&
                                    "bg-red-500/20",
                                )}
                              >
                                {line}
                              </div>
                            ))}
                          </pre>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="border rounded-lg p-6 text-center text-muted-foreground">
              No file changes
            </div>
          )}
        </section>
      </div>
    </div>
  );
};
