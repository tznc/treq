import { TriangleAlert } from "lucide-react";
import type { PrInfo } from "../lib/api";
import { cn } from "../lib/utils";

export type MergeLocation = "local" | "github";

/** Why GitHub may refuse the merge, from the PR's cached merge state. */
export const githubMergeWarnings = (
  pr: PrInfo,
  targetBranch: string,
): string[] => {
  const warnings: string[] = [];
  if (pr.base_ref_name !== targetBranch) {
    warnings.push(
      `The pull request targets ${pr.base_ref_name}, but this workspace targets ${targetBranch}. Retarget one of them before merging on GitHub.`,
    );
  }
  if (pr.is_draft) {
    warnings.push(
      "This pull request is a draft. Mark it ready for review before merging.",
    );
  }
  switch (pr.merge_state_status) {
    case "DIRTY":
      warnings.push(
        "This pull request has merge conflicts on GitHub. Resolve them before merging.",
      );
      break;
    case "BLOCKED":
      warnings.push(
        "GitHub is blocking this merge: required checks or reviews are not satisfied.",
      );
      break;
    case "BEHIND":
      warnings.push(
        `The pull request branch is behind ${pr.base_ref_name}. GitHub may require updating it first.`,
      );
      break;
    case "UNSTABLE":
      warnings.push("Some checks are failing on this pull request.");
      break;
    default:
      break;
  }
  return warnings;
};

export const MergeLocationToggle = ({
  value,
  prNumber,
  onChange,
}: {
  value: MergeLocation;
  prNumber: number;
  onChange: (value: MergeLocation) => void;
}) => (
  <div
    role="radiogroup"
    aria-label="Merge location"
    className="inline-flex self-start rounded-md border overflow-hidden"
  >
    {(
      [
        ["local", "Locally"],
        ["github", `On GitHub (#${prNumber})`],
      ] as const
    ).map(([option, label]) => (
      <button
        key={option}
        type="button"
        role="radio"
        aria-checked={value === option}
        onClick={() => onChange(option)}
        className={cn(
          "px-3 py-1.5 text-sm transition-colors",
          value === option ? "bg-primary/10 font-medium" : "hover:bg-muted/50",
        )}
      >
        {label}
      </button>
    ))}
  </div>
);

export const MergeWarnings = ({ warnings }: { warnings: string[] }) => (
  <>
    {warnings.map((warning) => (
      <p
        key={warning}
        className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-sm"
      >
        <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
        {warning}
      </p>
    ))}
  </>
);
