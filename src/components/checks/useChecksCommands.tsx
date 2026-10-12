import type { ReactNode } from "react";
import { ListChecks, Play, RotateCw, ShieldCheck } from "lucide-react";
import type { JobResult, Workspace } from "../../lib/api-types";
import {
  checksTargetOf,
  useWorkspaceChecks,
} from "../../hooks/useWorkspaceChecks";
import { useToast } from "../ui/toast";

interface ChecksCommand {
  id: string;
  type: "action";
  label: string;
  description?: string;
  icon: ReactNode;
  onSelect: () => void;
}

/** Command palette entries for running the current workspace's checks. */
export function useChecksCommands(
  workspace: Workspace | null,
): ChecksCommand[] {
  const checks = useWorkspaceChecks(checksTargetOf(workspace));
  const { addToast } = useToast();
  if (!workspace) return [];

  const report = (label: string, run: Promise<JobResult[] | JobResult>) => {
    run
      .then((result) => {
        const results = Array.isArray(result) ? result : [result];
        const passed = results.every((job) => job.success);
        addToast({
          title: passed ? `${label} passed` : `${label} failed`,
          type: passed ? "success" : "error",
        });
      })
      .catch((error: unknown) =>
        addToast({
          title: `${label} could not run`,
          description: error instanceof Error ? error.message : String(error),
          type: "error",
        }),
      );
  };

  const commands: ChecksCommand[] = [];
  if (checks.workflows.length > 0 && !checks.trusted) {
    commands.push({
      id: "trust-repo-checks",
      type: "action",
      label: "Trust repository to run checks",
      description: "Allow workflows in .treq/workflows to run",
      icon: <ShieldCheck className="w-4 h-4" />,
      onSelect: () => void checks.trust(),
    });
  }
  if (checks.canRun && checks.workflows.length > 0) {
    commands.push({
      id: "run-all-checks",
      type: "action",
      label: "Run checks…",
      description: `Run all ${checks.workflows.length} workflows in this workspace`,
      icon: <ListChecks className="w-4 h-4" />,
      onSelect: () =>
        report(
          "Checks",
          checks.runAll().then((runs) => runs.flat()),
        ),
    });
    for (const wf of checks.workflows) {
      commands.push({
        id: `run-check-${wf.filename}`,
        type: "action",
        label: `Run check: ${wf.name}`,
        description: wf.filename,
        icon: <Play className="w-4 h-4" />,
        onSelect: () => report(wf.name, checks.runWorkflow(wf)),
      });
    }
  }
  if (checks.setupStatus?.configured) {
    commands.push({
      id: "rerun-setup-script",
      type: "action",
      label: "Re-run setup script",
      description: "Run the workspace setup script again",
      icon: <RotateCw className="w-4 h-4" />,
      onSelect: () => void checks.rerunSetup().catch(() => undefined),
    });
  }
  return commands;
}
