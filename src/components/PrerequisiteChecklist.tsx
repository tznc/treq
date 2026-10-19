import { openUrl } from "@tauri-apps/plugin-opener";
import {
  CheckCircle2,
  Circle,
  ExternalLink,
  RefreshCw,
  XCircle,
} from "lucide-react";
import useSWR from "swr";
import { checkPrerequisites } from "../lib/api";
import {
  PREREQUISITE_TOOLS,
  type PrerequisiteGroup,
  type PrerequisiteTool,
} from "../lib/prerequisites";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";

interface ToolRowProps {
  tool: PrerequisiteTool;
  /** `undefined` while the check is still running. */
  installed: boolean | undefined;
}

const StatusIcon = ({ tool, installed }: ToolRowProps) => {
  if (installed) {
    return <CheckCircle2 className="h-4 w-4 shrink-0 text-green-500" />;
  }
  if (installed === false && tool.group === "required") {
    return <XCircle className="h-4 w-4 shrink-0 text-destructive" />;
  }
  return <Circle className="h-4 w-4 shrink-0 text-muted-foreground" />;
};

const ToolRow = ({ tool, installed }: ToolRowProps) => (
  <li aria-label={tool.label} className="flex items-center gap-2 py-1">
    <StatusIcon tool={tool} installed={installed} />
    <div className="min-w-0 flex-1">
      <div className="text-sm">{tool.label}</div>
      {tool.note && (
        <div className="text-xs text-muted-foreground">{tool.note}</div>
      )}
    </div>
    {installed === undefined && (
      <span className="text-xs text-muted-foreground">Checking...</span>
    )}
    {installed === true && (
      <span className="text-xs text-muted-foreground">Installed</span>
    )}
    {installed === false && (
      <Button
        variant="link"
        size="sm"
        aria-label={`Install ${tool.label}`}
        className="h-auto gap-1 p-0 text-xs"
        onClick={() => void openUrl(tool.installUrl)}
      >
        Install
        <ExternalLink className="h-3 w-3" />
      </Button>
    )}
  </li>
);

const GROUP_LABELS: Record<PrerequisiteGroup, string> = {
  required: "Required",
  agent: "Agent CLI (install at least one)",
  optional: "Optional",
};

/**
 * Shows which of Git, the agent CLIs and the GitHub CLI are installed, with an
 * install link for each missing one.
 */
export const PrerequisiteChecklist = ({
  title = "Before you start",
}: {
  title?: string;
}) => {
  const { data, isValidating, mutate } = useSWR(
    ["prerequisites"],
    checkPrerequisites,
  );
  const installed = data && new Map(data.map((s) => [s.binary, s.installed]));
  const noAgent =
    installed !== undefined &&
    !PREREQUISITE_TOOLS.some(
      (t) => t.group === "agent" && installed.get(t.binary),
    );

  return (
    <section aria-label="Setup checklist" className="w-full">
      <div className="mb-1 flex items-center justify-between">
        <h4 className="text-sm font-medium">{title}</h4>
        <Button
          variant="ghost"
          size="sm"
          className="-mr-2 h-6 gap-1 px-2 text-xs"
          disabled={isValidating}
          onClick={() => void mutate()}
        >
          <RefreshCw
            className={cn("h-3 w-3", isValidating && "animate-spin")}
          />
          Check again
        </Button>
      </div>
      {(Object.keys(GROUP_LABELS) as PrerequisiteGroup[]).map((group) => (
        <div key={group} className="mt-2">
          <div
            className={cn(
              "text-xs text-muted-foreground",
              group === "agent" && noAgent && "text-destructive",
            )}
          >
            {GROUP_LABELS[group]}
          </div>
          <ul>
            {PREREQUISITE_TOOLS.filter((t) => t.group === group).map((tool) => (
              <ToolRow
                key={tool.binary}
                tool={tool}
                installed={installed?.get(tool.binary)}
              />
            ))}
          </ul>
        </div>
      ))}
    </section>
  );
};
