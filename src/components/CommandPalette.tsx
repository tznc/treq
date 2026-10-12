import { Command } from "cmdk";
import { openUrl } from "@tauri-apps/plugin-opener";
import { BranchSwitcher } from "./BranchSwitcher";
import { WorkspaceDeletion } from "./WorkspaceDeletion";
import { FilePicker } from "./FilePicker";
import { ClaudeIcon } from "./icons/AgentIcons";
import { CmdkFooter } from "./ui/cmdk-footer";
import { Workspace } from "../lib/api";
import { getFullWorkspacePath } from "../lib/utils";
import { usePrInfoViaGh } from "../hooks/useMergeQueueStatus";
import { useChecksCommands } from "./checks/useChecksCommands";
import {
  AppWindow,
  ChevronsUpDown,
  ExternalLink,
  FileSearch,
  GitBranch,
  History,
  Home,
  Archive,
  FolderOpen,
  Maximize2,
  Plus,
  Settings,
  Terminal as TerminalIcon,
  Trash2,
} from "lucide-react";

interface CommandItem {
  id: string;
  type: "action" | "workspace" | "session";
  label: string;
  description?: string;
  icon: React.ReactNode;
  onSelect: () => void;
}

interface CommandPaletteProps {
  // Command Palette
  showCommandPalette: boolean;
  onCommandPaletteChange: (open: boolean) => void;
  workspaces: Workspace[];
  onNavigateToDashboard: () => void;
  onNavigateToSettings: () => void;
  onOpenRepository?: () => void;
  onOpenRepositoryInNewWindow?: () => void;
  onOpenBranchSwitcher: () => void;
  onOpenFilePicker: () => void;
  onOpenWorkspacePicker: () => void;
  onOpenWorkspaceDeletion: () => void;
  onCreateStackedWorkspace: () => void;
  onToggleTerminal?: () => void;
  onMaximizeTerminal?: () => void;
  onStartAgentWithPrompt?: () => void;
  onStartAgentTerminal?: () => void;
  onCreateShellTerminal?: () => void;
  onOpenPromptHistory?: () => void;
  onOpenStash?: () => void;
  hasSelectedWorkspace: boolean;

  // Branch Switcher
  showBranchSwitcher: boolean;
  onBranchSwitcherChange: (open: boolean) => void;
  onBranchChanged: (branchName: string) => void;

  // Workspace Deletion
  showWorkspaceDeletion: boolean;
  onWorkspaceDeletionChange: (open: boolean) => void;
  currentWorkspace: Workspace | null;
  onDeleteWorkspace: (workspace: Workspace) => void;

  // File Picker
  showFilePicker: boolean;
  onFilePickerChange: (open: boolean) => void;
  onFileSelected: (filePath: string) => void;
  selectedWorkspaceId: number | null;

  // Common
  repoPath: string;
}

export const CommandPalette: React.FC<CommandPaletteProps> = ({
  showCommandPalette,
  onCommandPaletteChange,
  workspaces,
  onNavigateToDashboard,
  onNavigateToSettings,
  onOpenRepository,
  onOpenRepositoryInNewWindow,
  onOpenBranchSwitcher,
  onOpenFilePicker,
  onOpenWorkspacePicker,
  onOpenWorkspaceDeletion,
  onCreateStackedWorkspace,
  onToggleTerminal,
  onMaximizeTerminal,
  onStartAgentWithPrompt,
  onStartAgentTerminal,
  onCreateShellTerminal,
  onOpenPromptHistory,
  onOpenStash,
  hasSelectedWorkspace,
  showBranchSwitcher,
  onBranchSwitcherChange,
  onBranchChanged,
  showWorkspaceDeletion,
  onWorkspaceDeletionChange,
  currentWorkspace,
  onDeleteWorkspace,
  showFilePicker,
  onFilePickerChange,
  onFileSelected,
  selectedWorkspaceId,
  repoPath,
}) => {
  const { data: workspacePrInfo } = usePrInfoViaGh(
    repoPath || undefined,
    currentWorkspace?.branch_name,
  );

  const checksCommands = useChecksCommands(currentWorkspace);

  // Build command items
  const items: CommandItem[] = (() => {
    const result: CommandItem[] = [];

    result.push({
      id: "dashboard",
      type: "action",
      label: "Go to Home",
      icon: <Home className="w-4 h-4" />,
      onSelect: onNavigateToDashboard,
    });

    result.push({
      id: "settings",
      type: "action",
      label: "Go to Settings",
      icon: <Settings className="w-4 h-4" />,
      onSelect: onNavigateToSettings,
    });

    if (onOpenRepository) {
      result.push({
        id: "open-repository",
        type: "action",
        label: "Open Repository",
        description: "Open a different repository",
        icon: <FolderOpen className="w-4 h-4" />,
        onSelect: onOpenRepository,
      });
    }

    if (onOpenRepositoryInNewWindow) {
      result.push({
        id: "open-repository-new-window",
        type: "action",
        label: "Open Repository in New Window",
        description: "Open a repository in a separate window",
        icon: <AppWindow className="w-4 h-4" />,
        onSelect: onOpenRepositoryInNewWindow,
      });
    }

    if (repoPath && onOpenBranchSwitcher) {
      result.push({
        id: "switch-branch",
        type: "action",
        label: "Switch Branch",
        description: "Checkout a different branch in main tree",
        icon: <GitBranch className="w-4 h-4" />,
        onSelect: onOpenBranchSwitcher,
      });
    }

    if (repoPath && onOpenFilePicker) {
      result.push({
        id: "search-files",
        type: "action",
        label: "Search Files",
        description: "Jump to a file in the repository",
        icon: <FileSearch className="w-4 h-4" />,
        onSelect: onOpenFilePicker,
      });
    }

    if (repoPath && onOpenWorkspacePicker) {
      result.push({
        id: "go-to-workspace",
        type: "action",
        label: "Go to workspace",
        description: "Open and filter workspaces",
        icon: <GitBranch className="w-4 h-4" />,
        onSelect: onOpenWorkspacePicker,
      });
    }

    if (repoPath && onCreateStackedWorkspace) {
      result.push({
        id: "create-workspace",
        type: "action",
        label: "Create Workspace",
        description: "Create a new workspace",
        icon: <Plus className="w-4 h-4" />,
        onSelect: onCreateStackedWorkspace,
      });
    }

    if (repoPath && onOpenWorkspaceDeletion) {
      result.push({
        id: "delete-workspace",
        type: "action",
        label: "Delete Workspace",
        description: "Delete a workspace",
        icon: <Trash2 className="w-4 h-4" />,
        onSelect: onOpenWorkspaceDeletion,
      });
    }

    if (repoPath && onStartAgentWithPrompt) {
      result.push({
        id: "start-agent-with-prompt",
        type: "action",
        label: "Start a new agent session with a prompt",
        description: "Write a prompt and choose its workspace",
        icon: <ClaudeIcon className="w-4 h-4" />,
        onSelect: onStartAgentWithPrompt,
      });
    }

    if (repoPath && onStartAgentTerminal) {
      result.push({
        id: "start-agent-terminal",
        type: "action",
        label: "Start a new agent terminal",
        description: "Start the default agent immediately",
        icon: <ClaudeIcon className="w-4 h-4" />,
        onSelect: onStartAgentTerminal,
      });
    }

    if (repoPath && onOpenPromptHistory) {
      result.push({
        id: "open-prompt-history",
        type: "action",
        label: "View Prompt History",
        description: "See every prompt ever sent, labeled by workspace",
        icon: <History className="w-4 h-4" />,
        onSelect: onOpenPromptHistory,
      });
    }

    if (repoPath && onOpenStash) {
      result.push({
        id: "open-stash",
        type: "action",
        label: "View Stashed Changes",
        description: "Browse and apply immutable stashed change sets",
        icon: <Archive className="w-4 h-4" />,
        onSelect: onOpenStash,
      });
    }

    if (hasSelectedWorkspace) {
      if (onToggleTerminal) {
        result.push({
          id: "toggle-terminal",
          type: "action",
          label: "Toggle Terminal",
          description: "Show or hide the terminal pane",
          icon: <ChevronsUpDown className="w-4 h-4" />,
          onSelect: onToggleTerminal,
        });
      }

      if (onMaximizeTerminal) {
        result.push({
          id: "maximize-terminal",
          type: "action",
          label: "Maximize Terminal",
          description: "Toggle maximize/restore terminal pane",
          icon: <Maximize2 className="w-4 h-4" />,
          onSelect: onMaximizeTerminal,
        });
      }

      if (onCreateShellTerminal) {
        result.push({
          id: "new-shell-terminal",
          type: "action",
          label: "New Shell Terminal",
          description: "Create a new shell session",
          icon: <TerminalIcon className="w-4 h-4" />,
          onSelect: onCreateShellTerminal,
        });
      }
    }

    result.push(...checksCommands);

    if (currentWorkspace && workspacePrInfo) {
      result.push({
        id: "open-workspace-pr-browser",
        type: "action",
        label: "Open Workspace PR in Browser",
        description: `Open PR #${workspacePrInfo.number} on GitHub`,
        icon: <ExternalLink className="w-4 h-4" />,
        onSelect: () => openUrl(workspacePrInfo.url),
      });

      result.push({
        id: "open-workspace-pr-current-window",
        type: "action",
        label: "Open Workspace PR",
        description: `Navigate to PR #${workspacePrInfo.number}`,
        icon: <AppWindow className="w-4 h-4" />,
        onSelect: () => {
          window.location.href = workspacePrInfo.url;
        },
      });
    }

    // Wrap all onSelect handlers to close the dialog after execution
    return result.map((item) => ({
      ...item,
      onSelect: () => {
        item.onSelect();
        onCommandPaletteChange(false);
      },
    }));
  })();

  // Render a command item
  const renderItem = (item: CommandItem) => (
    <Command.Item
      key={item.id}
      value={item.label}
      onSelect={item.onSelect}
      className="px-3 py-1.5 mx-2 rounded-md flex items-center gap-3 cursor-pointer text-foreground aria-selected:bg-accent/50 aria-selected:text-foreground data-[disabled='true']:opacity-50 data-[disabled='true']:pointer-events-none hover:bg-accent/30 transition-colors"
    >
      <span className="text-muted-foreground">{item.icon}</span>
      <div className="flex-1 min-w-0">
        <div className="truncate text-sm font-medium">{item.label}</div>
        {item.description && (
          <div className="truncate text-sm text-muted-foreground">
            {item.description}
          </div>
        )}
      </div>
    </Command.Item>
  );

  return (
    <>
      {/* Command Palette */}
      <Command.Dialog
        open={showCommandPalette}
        onOpenChange={onCommandPaletteChange}
        label="Command Menu"
        className="[&_[cmdk-root]]:bg-background [&_[cmdk-root]]:text-foreground"
      >
        <span className="sr-only">
          <h2>Command Menu</h2>
          <p>Command menu</p>
        </span>
        <div
          data-testid="modal"
          className="bg-background text-foreground rounded-xl border border-border shadow-2xl w-[40vw] max-w-none overflow-hidden"
        >
          <div className="flex items-center border-b border-border px-3 bg-background">
            <Command.Input
              placeholder="Type a command or search..."
              className="border-0 focus-visible:ring-0 focus-visible:ring-offset-0 h-12 flex-1 bg-transparent outline-none text-sm placeholder:text-muted-foreground text-foreground"
            />
          </div>

          <Command.List className="max-h-[300px] overflow-y-auto py-2">
            <Command.Empty>
              <div className="px-4 py-8 text-center text-muted-foreground text-sm">
                No results found
              </div>
            </Command.Empty>

            {items.map(renderItem)}
          </Command.List>

          <CmdkFooter />
        </div>
      </Command.Dialog>

      {/* Other Modals */}
      {repoPath && (
        <>
          <BranchSwitcher
            open={showBranchSwitcher}
            onOpenChange={onBranchSwitcherChange}
            repoPath={repoPath}
            onBranchChanged={onBranchChanged}
          />

          <WorkspaceDeletion
            open={showWorkspaceDeletion}
            onOpenChange={onWorkspaceDeletionChange}
            workspaces={workspaces}
            repoPath={repoPath}
            currentWorkspace={currentWorkspace}
            onDeleteWorkspace={onDeleteWorkspace}
          />

          <FilePicker
            open={showFilePicker}
            onOpenChange={onFilePickerChange}
            repoPath={repoPath}
            workspaceId={selectedWorkspaceId}
            workspacePath={
              currentWorkspace
                ? getFullWorkspacePath(currentWorkspace)
                : repoPath
            }
            onFileSelect={onFileSelected}
          />
        </>
      )}
    </>
  );
};
