import { ArrowDown, GitBranch, GitPullRequest, Layers } from "lucide-react";
import type { MergeLandingPlan, PrInfo } from "../lib/api";

/** "a", "a and b", "a, b and c". */
const joinBranches = (branches: string[]) =>
  branches.length <= 1
    ? (branches[0] ?? "")
    : `${branches.slice(0, -1).join(", ")} and ${branches[branches.length - 1]}`;

const BranchChip = ({ name }: { name: string }) => (
  <span className="inline-flex items-center gap-1 rounded-md border bg-muted/50 px-2 py-0.5 font-mono text-sm">
    <GitBranch className="h-3 w-3 shrink-0 text-muted-foreground" />
    {name}
  </span>
);

export interface MergeLandingPanelProps {
  plan: MergeLandingPlan;
  /** Set when merging on GitHub. */
  pr: PrInfo | null;
  /** The target is pushed to origin after a local merge. */
  pushed: boolean;
  /** Shown when the workspace is stacked and merging locally. */
  rollup: {
    checked: boolean;
    rootBranch: string;
    onChange: (checked: boolean) => void;
  } | null;
}

/** Shows which branches land in which branch, and what happens to the stack. */
export const MergeLandingPanel = ({
  plan,
  pr,
  pushed,
  rollup,
}: MergeLandingPanelProps) => {
  const landed = plan.landed_branches;
  return (
    <section aria-labelledby="merge-landing-heading" className="space-y-3">
      <h2 id="merge-landing-heading" className="text-sm font-semibold">
        What lands where
      </h2>
      <div className="rounded-lg border p-4 space-y-3">
        <div className="flex flex-col items-start gap-1.5">
          <div className="flex flex-wrap items-center gap-1.5">
            {landed.length > 1 ? (
              <Layers
                className="h-4 w-4 text-muted-foreground"
                aria-hidden="true"
              />
            ) : null}
            {landed.map((branch) => (
              <BranchChip key={branch} name={branch} />
            ))}
          </div>
          <div className="flex items-center gap-1.5 pl-2 text-sm text-muted-foreground">
            <ArrowDown className="h-3.5 w-3.5" aria-hidden="true" />
            {pr ? (
              <span className="inline-flex items-center gap-1">
                <GitPullRequest className="h-3.5 w-3.5" aria-hidden="true" />
                PR #{pr.number} on GitHub
              </span>
            ) : (
              <span>
                {pushed ? "merged locally, then pushed" : "merged locally"}
              </span>
            )}
          </div>
          <BranchChip name={plan.target_branch} />
        </div>

        <p className="text-sm">
          {landed.length > 1
            ? `${joinBranches(landed)} land in ${plan.target_branch}.`
            : `${landed[0]} lands in ${plan.target_branch}.`}
          {pr ? ` Then ${plan.target_branch} is synced from origin.` : null}
        </p>

        {plan.retargeted.length > 0 ? (
          <ul className="space-y-1 text-sm text-muted-foreground">
            {plan.retargeted.map((r) => (
              <li key={r.workspace_id}>
                {r.branch_name} is retargeted onto {r.to_branch} (was{" "}
                {r.from_branch})
              </li>
            ))}
          </ul>
        ) : null}

        {rollup ? (
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={rollup.checked}
              onChange={(e) => rollup.onChange(e.target.checked)}
            />
            Merge the whole stack into {rollup.rootBranch}
          </label>
        ) : null}
      </div>
    </section>
  );
};
