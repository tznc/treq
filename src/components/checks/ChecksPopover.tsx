import { type ReactNode, useState } from "react";
import { Loader2, Play, RotateCw, ShieldCheck } from "lucide-react";
import type { PrCiStatus, WorkflowInfo } from "../../lib/api-types";
import type { ChecksSummary } from "../../lib/checks-summary";
import { ciStatusStyle } from "../../lib/ci-status";
import type { WorkspaceChecks } from "../../hooks/useWorkspaceChecks";
import { cn } from "../../lib/utils";
import { useGpuiPart } from "../../lib/gpui-parts";
import { CiChecksList } from "../CiStatusButton";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { ChecksStatusIcon } from "./ChecksStatusIcon";

type CombinedStatus = "passed" | "failed" | "running" | undefined;

function treqLabel(summary: ChecksSummary): string {
  if (summary.state === "running") return "running";
  if (summary.state === "not-run") return "not run";
  return `${summary.passed}/${summary.total}`;
}

const CI_TO_STATUS: Record<string, CombinedStatus> = {
  success: "passed",
  failure: "failed",
  pending: "running",
};

// Failed beats running beats passed; nothing run yet shows the neutral icon.
function combinedStatus(
  summary: ChecksSummary,
  ci: PrCiStatus | null,
): CombinedStatus {
  const states: CombinedStatus[] = [];
  if (summary.state === "passed" || summary.state === "failed") {
    states.push(summary.state);
  } else if (summary.state === "running") {
    states.push("running");
  } else if (summary.state === "not-run") {
    states.push(undefined);
  }
  if (ci) states.push(CI_TO_STATUS[ci.state] ?? "running");
  if (states.includes("failed")) return "failed";
  if (states.includes("running")) return "running";
  if (states.length > 0 && states.every((s) => s === "passed")) return "passed";
  return undefined;
}

/**
 * One header button for every check on the workspace: Treq workflow checks
 * with trust and run actions, and the GitHub checks of the branch's PR.
 * Renders nothing when there are neither workflows nor PR checks.
 */
export function ChecksPopover({
  checks,
  ciStatus,
  onOpenLogs,
}: {
  checks: WorkspaceChecks;
  ciStatus?: PrCiStatus | null;
  onOpenLogs?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const { summary } = checks;
  const ci = ciStatus && ciStatus.total > 0 ? ciStatus : null;
  const hasTreq = summary.state !== "none";
  const status = combinedStatus(summary, ci);
  // While it is open, the screen hands the header back to React to draw it.
  useGpuiPart(
    "checks",
    hasTreq || ci
      ? {
          open,
          status: status ?? null,
          treq: hasTreq ? treqLabel(summary) : null,
          github: ci
            ? { label: `GH ${ci.passed}/${ci.total}`, state: ci.state }
            : null,
        }
      : null,
    { open: () => setOpen(true) },
  );
  if (!hasTreq && !ci) return null;

  const parts = [
    hasTreq && `Treq ${treqLabel(summary)}`,
    ci && `GitHub ${ci.passed}/${ci.total}`,
  ].filter(Boolean);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5 px-2"
          data-testid="workspace-checks-button"
          data-checks-state={hasTreq ? summary.state : "none"}
          aria-label={`Checks: ${parts.join(", ")}`}
        >
          <ChecksStatusIcon status={status} className="h-3.5 w-3.5" />
          Checks
          {hasTreq && (
            <span className="text-muted-foreground">{treqLabel(summary)}</span>
          )}
          {ci && (
            <span
              className={cn(
                "border-l pl-1.5",
                ciStatusStyle(ci.state).textClassName,
              )}
              title="GitHub checks"
            >
              GH {ci.passed}/{ci.total}
            </span>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        data-testid="checks-popover"
        align="end"
        className="w-80 p-2"
      >
        {hasTreq && (
          <TreqChecksSection
            checks={checks}
            onOpenLogs={
              onOpenLogs &&
              (() => {
                setOpen(false);
                onOpenLogs();
              })
            }
          />
        )}
        {ci && (
          <section
            aria-label="GitHub checks"
            className={cn(hasTreq && "mt-2 border-t pt-2")}
          >
            <SectionTitle>
              GitHub checks · {ci.passed}/{ci.total}
            </SectionTitle>
            <CiChecksList ciStatus={ci} />
          </section>
        )}
      </PopoverContent>
    </Popover>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div className="px-2 pb-1 text-xs font-medium text-muted-foreground">
      {children}
    </div>
  );
}

function TreqChecksSection({
  checks,
  onOpenLogs,
}: {
  checks: WorkspaceChecks;
  onOpenLogs?: () => void;
}) {
  const setup = checks.setupStatus;
  return (
    <section aria-label="Treq checks">
      <div className="flex items-center justify-between">
        <SectionTitle>Treq checks</SectionTitle>
        {onOpenLogs && (
          <button
            type="button"
            className="px-2 pb-1 text-xs text-primary hover:underline"
            onClick={onOpenLogs}
          >
            View logs
          </button>
        )}
      </div>
      {checks.workflows.map((wf) => (
        <WorkflowRow key={wf.filename} workflow={wf} checks={checks} />
      ))}
      <div className="flex items-center gap-2 px-2 pt-2">
        {!checks.trusted && (
          <Button
            size="sm"
            variant="outline"
            className="border-amber-300 text-amber-800 dark:border-amber-800 dark:text-amber-200"
            title="Trust this repository to enable running workflow checks"
            onClick={() => void checks.trust()}
          >
            <ShieldCheck className="h-3 w-3 mr-1" />
            Trust Repository
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          aria-label="Run all checks"
          disabled={!checks.canRun || checks.summary.state === "running"}
          onClick={() => void checks.runAll().catch(() => undefined)}
        >
          {checks.summary.state === "running" ? (
            <Loader2 className="h-3 w-3 mr-1 animate-spin" />
          ) : (
            <Play className="h-3 w-3 mr-1" />
          )}
          Run all
        </Button>
        {setup?.configured && (
          <Button
            size="sm"
            variant="ghost"
            className="ml-auto"
            aria-label="Re-run setup script"
            title={`Setup script: ${setup.status}`}
            disabled={checks.rerunningSetup || checks.setupRunning}
            onClick={() => void checks.rerunSetup()}
          >
            {checks.rerunningSetup || checks.setupRunning ? (
              <Loader2 className="h-3 w-3 mr-1 animate-spin" />
            ) : (
              <ChecksStatusIcon
                status={setup.status}
                className="h-3 w-3 mr-1"
              />
            )}
            Setup
          </Button>
        )}
      </div>
    </section>
  );
}

function WorkflowRow({
  workflow: wf,
  checks,
}: {
  workflow: WorkflowInfo;
  checks: WorkspaceChecks;
}) {
  const latestRun = checks.latestRuns[wf.filename];
  const running = checks.isRunning(wf.filename);
  const status = running ? "running" : latestRun?.status;
  const detail = running
    ? "running"
    : latestRun
      ? `run #${latestRun.id} ${latestRun.status}`
      : "not run";
  const verb = latestRun ? "Re-run" : "Run";
  return (
    <div
      data-testid="checks-popover-workflow"
      className="flex items-center gap-2 py-1 px-2"
    >
      <ChecksStatusIcon status={status} />
      <span className="flex-1 min-w-0 truncate">{wf.name}</span>
      <span className="shrink-0 text-sm text-muted-foreground">{detail}</span>
      <Button
        size="sm"
        variant="ghost"
        className="h-6 w-6 p-0"
        aria-label={`${verb} ${wf.name}`}
        title={`${verb} ${wf.name}`}
        disabled={!checks.canRun || running}
        onClick={() => void checks.runWorkflow(wf).catch(() => undefined)}
      >
        {latestRun ? (
          <RotateCw className="h-3 w-3" />
        ) : (
          <Play className="h-3 w-3" />
        )}
      </Button>
    </div>
  );
}
