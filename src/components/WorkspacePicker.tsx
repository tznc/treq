import { Command } from "cmdk";
import { GitBranch } from "lucide-react";
import { Session, Workspace } from "../lib/api";

interface WorkspacePickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaces: Workspace[];
  sessions: Session[];
  workspaceChangeCounts: Map<number, number> | undefined;
  onSelect: (workspace: Workspace) => void;
}

export const WorkspacePicker: React.FC<WorkspacePickerProps> = ({
  open,
  onOpenChange,
  workspaces,
  sessions,
  workspaceChangeCounts,
  onSelect,
}) => {
  const handleSelect = (workspace: Workspace) => {
    onSelect(workspace);
    onOpenChange(false);
  };

  const getWorkspaceDescription = (workspace: Workspace) => {
    const workspaceSessions = sessions.filter(
      (s) => s.workspace_id === workspace.id,
    );
    const agentCount = workspaceSessions.length;
    const changeCount = workspaceChangeCounts?.get(workspace.id) ?? 0;

    const parts: string[] = [];
    parts.push(`${agentCount} agent${agentCount !== 1 ? "s" : ""}`);
    parts.push(`0 shells`);
    parts.push(`${changeCount} change${changeCount !== 1 ? "s" : ""}`);

    return parts.join(", ");
  };

  return (
    <Command.Dialog
      open={open}
      onOpenChange={onOpenChange}
      label="Go to Workspace"
    >
      <span className="sr-only">
        <h2>Go to Workspace</h2>
        <p>Go to workspace</p>
      </span>
      <div
        data-testid="modal"
        className="bg-popover text-popover-foreground rounded-xl border border-border/50 shadow-2xl w-[40vw] max-w-none overflow-hidden"
      >
        {/* Search Input */}
        <div className="flex items-center border-b border-border px-3">
          <GitBranch className="w-4 h-4 text-muted-foreground mr-2" />
          <Command.Input
            placeholder="Search workspaces..."
            className="border-0 focus-visible:ring-0 focus-visible:ring-offset-0 h-12 flex-1 bg-transparent outline-none text-sm placeholder:text-muted-foreground"
          />
        </div>

        {/* Results List */}
        <Command.List className="max-h-[400px] overflow-y-auto py-2">
          <Command.Empty>
            <div className="px-4 py-8 text-center text-muted-foreground text-sm">
              No workspaces found
            </div>
          </Command.Empty>

          {workspaces.map((workspace) => (
            <Command.Item
              key={workspace.id}
              value={workspace.branch_name}
              onSelect={() => handleSelect(workspace)}
              className="px-3 py-2 mx-2 rounded-md flex items-center gap-3 cursor-pointer aria-selected:bg-accent aria-selected:text-accent-foreground hover:bg-accent/50 transition-colors"
            >
              <GitBranch className="w-4 h-4 text-muted-foreground" />
              <div className="flex-1 min-w-0">
                <div className="truncate text-sm font-medium">
                  {workspace.branch_name}
                </div>
                <div className="truncate text-sm text-muted-foreground">
                  {getWorkspaceDescription(workspace)}
                </div>
              </div>
            </Command.Item>
          ))}
        </Command.List>

        {/* Footer with keyboard hints */}
        <div className="border-t border-border px-3 py-2 flex items-center justify-between text-sm text-muted-foreground">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">↑↓</kbd>{" "}
              Navigate
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">↵</kbd>{" "}
              Select
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">Esc</kbd>{" "}
              Close
            </span>
          </div>
        </div>
      </div>
    </Command.Dialog>
  );
};
