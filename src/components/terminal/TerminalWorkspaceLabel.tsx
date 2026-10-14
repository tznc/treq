import { Label } from "../ui/label";
import { cn } from "../../lib/utils";

interface TerminalWorkspaceLabelProps {
  workspaceName: string;
  onNavigate?: () => void;
  className?: string;
}

export function TerminalWorkspaceLabel({
  workspaceName,
  onNavigate,
  className,
}: TerminalWorkspaceLabelProps) {
  return (
    <Label
      data-testid="terminal-workspace-label"
      title={workspaceName}
      onClick={(event) => {
        if (!onNavigate) return;
        event.stopPropagation();
        onNavigate();
      }}
      className={cn(
        "truncate max-w-[200px] rounded-sm bg-muted/60 px-1 py-0.5 text-xs font-mono font-medium leading-none text-muted-foreground",
        onNavigate && "cursor-pointer hover:bg-muted",
        className,
      )}
    >
      {workspaceName}
    </Label>
  );
}
