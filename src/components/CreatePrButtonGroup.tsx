import { invalidateQueries } from "../lib/swr-cache";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronDown, Github, Loader2 } from "lucide-react";
import { useState } from "react";
import { useCreateWorkspacePr } from "../hooks/useCreateWorkspacePr";
import { useGitRemoteInfo, usePrInfoViaGh } from "../hooks/useMergeQueueStatus";
import { getWorkspaces, pushWorkspaceToRemote } from "../lib/api";
import type { Workspace } from "../lib/api-types";
import {
  buildGitHubComparePrUrl,
  deriveConventionalPrTitle,
} from "../lib/github-pr";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { useToast } from "./ui/toast";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";

interface CreatePrButtonGroupProps {
  repoPath: string;
  workspace: Workspace;
  baseBranch: string;
  hasCommits: boolean;
  needsPush: boolean;
}

export function CreatePrButtonGroup({
  repoPath,
  workspace,
  baseBranch,
  hasCommits,
  needsPush,
}: CreatePrButtonGroupProps) {
  const { addToast } = useToast();
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const { data: prInfo, isLoading: prInfoLoading } = usePrInfoViaGh(
    repoPath,
    workspace.branch_name,
  );
  const [pushingManually, setPushingManually] = useState(false);
  const title = deriveConventionalPrTitle(
    workspace.title ?? "",
    workspace.branch_name,
  );
  const body = workspace.description ?? "";

  const { createPr: createWorkspacePr, isPending: createPending } =
    useCreateWorkspacePr(repoPath, workspace.id);

  // `gh pr view <branch>` also returns closed and merged PRs. Only an open
  // PR blocks a new one; otherwise the branch can get a fresh PR.
  if (!remoteInfo || prInfoLoading || prInfo?.state === "OPEN") {
    return null;
  }

  // Agents rewrite the description with `treq desc` while the view is open,
  // so read it at click time rather than from the polled workspace list.
  const latestBody = async () => {
    const fresh = await getWorkspaces(repoPath).catch(() => null);
    const current = fresh?.find((w) => w.id === workspace.id);
    return current ? (current.description ?? "") : body;
  };

  const createPr = async (draft: boolean) =>
    createWorkspacePr({
      repoFullName: remoteInfo.full_name,
      branchName: workspace.branch_name,
      baseBranch,
      body: await latestBody(),
      draft,
      prepare: async () => {
        if (needsPush) {
          await pushWorkspaceToRemote(repoPath, workspace.id);
        }
        return title;
      },
    });

  const openManual = async () => {
    setPushingManually(true);
    try {
      if (needsPush) {
        await pushWorkspaceToRemote(repoPath, workspace.id);
        void invalidateQueries();
      }
      openUrl(
        buildGitHubComparePrUrl({
          owner: remoteInfo.owner,
          repo: remoteInfo.repo,
          baseBranch,
          headBranch: workspace.branch_name,
          title,
          body: await latestBody(),
        }),
      );
    } catch (err) {
      addToast({
        title: "Failed to push branch",
        description: (err as Error).message,
        type: "error",
      });
    } finally {
      setPushingManually(false);
    }
  };

  const creating = createPending || pushingManually;
  const pushAndCreate = needsPush;
  const disabled = creating || !hasCommits;
  const noCommitsTooltip =
    "Make a commit before creating a pull request. Uncommitted working-copy changes don't count.";

  return (
    <TooltipProvider delay={200}>
      <div className="inline-flex items-center">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="default"
              size="sm"
              className="gap-1 rounded-r-none bg-[#24292f] text-white hover:bg-[#1b1f23] dark:border-white/30"
              disabled={disabled}
              onClick={() => void createPr(false)}
            >
              {creating ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Github className="w-4 h-4" />
              )}
              {creating
                ? pushAndCreate
                  ? "Pushing & creating…"
                  : "Creating…"
                : "Create PR"}
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {!hasCommits
              ? noCommitsTooltip
              : pushAndCreate
                ? "Push this branch and create a pull request on GitHub"
                : "Create a pull request on GitHub"}
          </TooltipContent>
        </Tooltip>
        <DropdownMenu>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="default"
                  size="sm"
                  className="rounded-l-none border-l border-white/20 px-1.5 bg-[#24292f] text-white hover:bg-[#1b1f23] dark:border-white/30"
                  disabled={disabled}
                  aria-label="More Create PR options"
                >
                  <ChevronDown className="w-4 h-4" />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent>
              {!hasCommits ? noCommitsTooltip : "More pull request options"}
            </TooltipContent>
          </Tooltip>
          <DropdownMenuContent align="end" sideOffset={4}>
            <DropdownMenuItem
              disabled={disabled}
              onSelect={() => void createPr(true)}
            >
              Create draft PR
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={disabled}
              onSelect={() => void openManual()}
            >
              Create PR manually
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </TooltipProvider>
  );
}
