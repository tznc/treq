import {
  ChevronDown,
  GitBranch,
  GitCommitHorizontal,
  GitMerge,
  Loader2,
} from "lucide-react";
import type { MergeStrategy } from "../lib/api";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

export interface MergeActionButtonProps {
  strategy: MergeStrategy;
  availableMethods: MergeStrategy[];
  onStrategyChange: (strategy: MergeStrategy) => void;
  onMerge: () => void;
  disabled: boolean;
  merging: boolean;
  onGithub: boolean;
  commitCount: number | undefined;
  targetBranch: string;
}

const actionLabel = (strategy: MergeStrategy, onGithub: boolean) => {
  const base =
    strategy === "squash"
      ? "Squash and merge"
      : strategy === "rebase"
        ? "Rebase and merge"
        : onGithub
          ? "Merge"
          : "Confirm merge";
  return onGithub ? `${base} on GitHub` : base;
};

/** Split button: merge with the chosen strategy, or pick another one. */
export const MergeActionButton = ({
  strategy: mergeStrategy,
  availableMethods,
  onStrategyChange: setMergeStrategy,
  onMerge: handleMerge,
  disabled,
  merging,
  onGithub,
  commitCount,
  targetBranch,
}: MergeActionButtonProps) => (
  <div className="flex justify-start">
    <div className="inline-flex items-center rounded-md overflow-hidden">
      <Button
        onClick={handleMerge}
        disabled={disabled}
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
        {merging ? "Merging..." : actionLabel(mergeStrategy, onGithub)}
      </Button>
      <div
        className="w-px opacity-30"
        style={{ backgroundColor: "currentColor" }}
      />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            disabled={disabled}
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
            onValueChange={(value) => setMergeStrategy(value as MergeStrategy)}
          >
            {availableMethods.includes("merge") ? (
              <DropdownMenuRadioItem
                value="merge"
                className="flex-col items-start py-3"
              >
                <div className="font-semibold flex items-center gap-2">
                  <GitMerge className="h-4 w-4" />
                  Create a merge commit
                </div>
                <div className="text-xs text-muted-foreground mt-1 pl-6">
                  All commits from this branch will be added to the base branch
                  via a merge commit
                </div>
              </DropdownMenuRadioItem>
            ) : null}
            {availableMethods.includes("squash") ? (
              <DropdownMenuRadioItem
                value="squash"
                className="flex-col items-start py-3"
              >
                <div className="font-semibold flex items-center gap-2">
                  <GitCommitHorizontal className="h-4 w-4" />
                  Squash and merge
                </div>
                <div className="text-xs text-muted-foreground mt-1 pl-6">
                  The {commitCount || "multiple"} commits from this branch will
                  be combined into one commit
                </div>
              </DropdownMenuRadioItem>
            ) : null}
            {availableMethods.includes("rebase") ? (
              <DropdownMenuRadioItem
                value="rebase"
                className="flex-col items-start py-3"
              >
                <div className="font-semibold flex items-center gap-2">
                  <GitBranch className="h-4 w-4" />
                  Rebase and merge
                </div>
                <div className="text-xs text-muted-foreground mt-1 pl-6">
                  Commits will be rebased onto {targetBranch} with a linear
                  history
                </div>
              </DropdownMenuRadioItem>
            ) : null}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </div>
);
