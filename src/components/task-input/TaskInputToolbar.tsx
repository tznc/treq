import { ChevronDown, Loader2, Paperclip, Plus } from "lucide-react";
import { useId } from "react";
import { AgentIcon } from "../icons/AgentIcons";
import { Button } from "../ui/button";
import { Label } from "../ui/label";
import { Switch } from "../ui/switch";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../ui/tooltip";
import { cn } from "../../lib/utils";
import type { AgentPermissionMode } from "../../lib/agentDeepLink";
import { agentInfo, type AgentKind } from "../../lib/agents";
import { AgentOptions } from "../AgentOptions";

interface TaskInputToolbarProps {
  isEmpty: boolean;
  submitting: boolean;
  selectedAgent: AgentKind;
  configuredDefaultAgent: AgentKind;
  saveAsRepoDefault: boolean;
  showSaveAsRepoDefault: boolean;
  onOpenFilePicker: () => void;
  onAttachFromFinder: () => void;
  onAgentChange: (agent: AgentKind) => void;
  onSaveAsRepoDefaultChange: (checked: boolean) => void;
  /** The home view's "Run in a new workspace" toggle; omitted elsewhere. */
  newWorkspace?: {
    checked: boolean;
    onCheckedChange: (checked: boolean) => void;
  };
  onSubmit: (mode: AgentPermissionMode) => void;
}

export const TaskInputToolbar: React.FC<TaskInputToolbarProps> = ({
  isEmpty,
  submitting,
  selectedAgent,
  configuredDefaultAgent,
  saveAsRepoDefault,
  showSaveAsRepoDefault,
  onOpenFilePicker,
  onAttachFromFinder,
  onAgentChange,
  onSaveAsRepoDefaultChange,
  newWorkspace,
  onSubmit,
}) => {
  const supportsPlan = agentInfo(selectedAgent).hasPlanMode;
  const newWorkspaceSwitchId = useId();
  return (
    <div className="px-2 pb-2 pt-1 flex min-w-0 flex-wrap items-center justify-between gap-1">
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 text-xs px-2 gap-1"
          onClick={onOpenFilePicker}
        >
          <Plus className="w-4 h-4" />
          File
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 text-xs px-2 gap-1"
          onClick={onAttachFromFinder}
        >
          <Paperclip className="w-4 h-4" />
          Attach
        </Button>
        {newWorkspace && (
          <div className="ml-1 flex items-center gap-1.5">
            <Switch
              id={newWorkspaceSwitchId}
              className="h-4 w-7 [&>span]:h-3 [&>span]:w-3 [&>span[data-checked]]:translate-x-3"
              checked={newWorkspace.checked}
              onCheckedChange={newWorkspace.onCheckedChange}
            />
            <Label
              htmlFor={newWorkspaceSwitchId}
              className="cursor-pointer text-xs font-normal text-muted-foreground"
            >
              Run in a new workspace
            </Label>
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-wrap items-center justify-end gap-2">
        {showSaveAsRepoDefault && selectedAgent !== configuredDefaultAgent && (
          <label className="flex items-center gap-1.5 text-xs text-muted-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={saveAsRepoDefault}
              onChange={(e) => onSaveAsRepoDefaultChange(e.target.checked)}
              className="h-3.5 w-3.5 accent-blue-500"
            />
            Set as default for this repo
          </label>
        )}
        <div className="flex items-center gap-1.5">
          <AgentIcon
            agent={selectedAgent}
            className="w-3.5 h-3.5 text-muted-foreground shrink-0"
          />
          <select
            aria-label="Agent"
            value={selectedAgent}
            onChange={(e) => onAgentChange(e.target.value as AgentKind)}
            className="h-7 text-xs px-2 rounded-md border border-border bg-background text-foreground cursor-pointer focus:outline-none focus:ring-1 focus:ring-blue-400"
          >
            <AgentOptions />
          </select>
        </div>
        {/* Edit (or Run) is always the main action; plan mode sits in the
          menu, which only agents with a plan mode get. */}
        <div className="inline-flex items-center">
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  disabled={isEmpty || submitting}
                  aria-busy={submitting}
                  onClick={() => onSubmit("acceptEdits")}
                  className={cn(
                    "h-7 text-xs px-3",
                    supportsPlan && "rounded-r-none",
                  )}
                >
                  {submitting ? (
                    <>
                      <Loader2
                        className="w-3.5 h-3.5 animate-spin"
                        aria-hidden="true"
                      />
                      Starting…
                    </>
                  ) : selectedAgent === "claude" ? (
                    "Edit"
                  ) : (
                    "Run"
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top">
                <p className="text-xs">⌘+Enter</p>
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
          {supportsPlan && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  size="sm"
                  disabled={isEmpty || submitting}
                  aria-label="More submit options"
                  className="h-7 rounded-l-none border-l border-white/20 px-1.5"
                >
                  <ChevronDown className="w-3.5 h-3.5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" sideOffset={4} className="w-56">
                <DropdownMenuItem
                  onClick={() => onSubmit("plan")}
                  className="items-start"
                >
                  <div className="flex flex-col gap-0.5">
                    <span className="font-medium">Plan</span>
                    <span className="text-xs text-muted-foreground">
                      Plan the change first, without editing files
                    </span>
                  </div>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>
      </div>
    </div>
  );
};
