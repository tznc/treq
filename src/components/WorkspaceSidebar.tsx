import { Github, ListTodo, Search } from "lucide-react";
import { useState } from "react";
import type { SupportingRepo, Workspace } from "../lib/api";
import type { ChangeFilesMoveRequest } from "../lib/change-file-drag";
import { useRepositoryCacheKey } from "../lib/active-repository-context";
import { repoDisplayLabels } from "../lib/repo-labels";
import { usePreviewFeature } from "../stores/featurePreviewStore";
import { HomeRepoSidebarRow } from "./HomeRepoSidebarRow";
import { RepoWorkspaceList } from "./RepoWorkspaceList";
import type { TerminalSessionSummary } from "./terminal/types";
import { Kbd, KbdGroup } from "./ui/kbd";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarMenu,
} from "./ui/sidebar";
import { TooltipProvider } from "./ui/tooltip";
import { WorkspaceSidebarHeaderActions } from "./WorkspaceSidebarHeaderActions";
import { TRACKER_ICONS } from "./trackerIcons";
import { WorkspaceSidebarPanelButton } from "./WorkspaceSidebarPanelButton";
import { WorkspaceSidebarResizeHandle } from "./WorkspaceSidebarResizeHandle";

const collapsedReposKey = (mainRepoPath?: string) =>
  `treq.sidebar.collapsedRepos:${mainRepoPath ?? ""}`;

const readCollapsedRepos = (mainRepoPath?: string): Set<string> => {
  try {
    const raw = localStorage.getItem(collapsedReposKey(mainRepoPath));
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
};

const writeCollapsedRepos = (
  mainRepoPath: string | undefined,
  paths: Set<string>,
) => {
  try {
    localStorage.setItem(
      collapsedReposKey(mainRepoPath),
      JSON.stringify([...paths]),
    );
  } catch {
    // Collapsed state is a convenience; ignore storage failures.
  }
};

interface WorkspaceSidebarProps {
  /** Repository on screen: the main repository or a supporting one. */
  repoPath?: string;
  /** Repository that identifies the window. Defaults to `repoPath`. */
  mainRepoPath?: string;
  supportingRepos?: SupportingRepo[];
  onSelectRepoHome?: (repoPath: string) => void;
  onAddRepository?: () => void;
  onRemoveSupportingRepo?: (repoPath: string) => void;
  onLocateSupportingRepo?: (repoPath: string) => void;
  onOpenRepoSettings?: (repoPath: string) => void;
  homeRepoDisplayRef?: string | null;
  selectedWorkspaceId?: number | null;
  selectedWorkspaceIds?: Set<number>;
  onWorkspaceClick?: (workspace: Workspace) => void;
  onWorkspaceMultiSelect?: (
    workspace: Workspace | null,
    event: React.MouseEvent,
  ) => void;
  onBulkArchive?: () => void;
  onArchiveWorkspace?: (workspace: Workspace) => void;
  archivingWorkspaceIds?: Set<number>;
  exitingWorkspaceIds?: Set<number>;
  openSettings?: (tab?: string) => void;
  navigateToDashboard?: () => void;
  onOpenCommandPalette?: () => void;
  onOpenBranchSwitcher?: () => void;
  onOpenGitHub?: () => void;
  onOpenLinear?: () => void;
  onOpenTrello?: () => void;
  onOpenJira?: () => void;
  onOpenArtifacts?: () => void;
  currentPage?: string;
  onAddBefore?: (workspace: Workspace) => void;
  onAddAfter?: (workspace: Workspace) => void;
  onMoveWorkspace?: (workspace: Workspace, targetBranch: string | null) => void;
  onSelectStack?: (workspaceIds: Set<number>) => void;
  onStartAgent?: (workspace: Workspace) => void;
  onStartShell?: (workspace: Workspace) => void;
  onStartHomeAgent?: () => void;
  onStartHomeShell?: () => void;
  onStackHome?: () => void;
  terminalSessions?: TerminalSessionSummary[];
  onDropChangeFiles?: (request: ChangeFilesMoveRequest) => void;
}

export const WorkspaceSidebar: React.FC<WorkspaceSidebarProps> = ({
  repoPath,
  mainRepoPath,
  supportingRepos = [],
  onSelectRepoHome,
  onAddRepository,
  onRemoveSupportingRepo,
  onLocateSupportingRepo,
  onOpenRepoSettings,
  homeRepoDisplayRef,
  selectedWorkspaceId,
  selectedWorkspaceIds,
  onWorkspaceClick,
  onWorkspaceMultiSelect,
  onBulkArchive,
  onArchiveWorkspace,
  archivingWorkspaceIds,
  exitingWorkspaceIds,
  openSettings,
  onOpenCommandPalette,
  onOpenBranchSwitcher,
  onOpenGitHub,
  onOpenLinear,
  onOpenTrello,
  onOpenJira,
  onOpenArtifacts,
  currentPage,
  onAddAfter,
  onMoveWorkspace,
  onSelectStack,
  onStartAgent,
  onStartShell,
  onStartHomeAgent,
  onStartHomeShell,
  onStackHome,
  terminalSessions,
  onDropChangeFiles,
}) => {
  const contextCacheKey = useRepositoryCacheKey(repoPath);
  const linearIntegration = usePreviewFeature("linearIntegration");
  const homeRepoPath = mainRepoPath ?? repoPath;
  const presentSupportingRepos = supportingRepos.filter((repo) => repo.exists);
  const isMultiRepo = supportingRepos.length > 0;
  const labels = repoDisplayLabels([
    homeRepoPath ?? "",
    ...supportingRepos.map((repo) => repo.path),
  ]);
  const [collapsedRepos, setCollapsedRepos] = useState<Set<string>>(() =>
    readCollapsedRepos(homeRepoPath),
  );
  const toggleCollapsed = (path: string) => {
    setCollapsedRepos((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      writeCollapsedRepos(homeRepoPath, next);
      return next;
    });
  };

  const handleContainerClick = (e: React.MouseEvent) => {
    if (
      e.target === e.currentTarget &&
      selectedWorkspaceIds &&
      selectedWorkspaceIds.size > 0 &&
      onWorkspaceMultiSelect
    ) {
      onWorkspaceMultiSelect(
        null as Parameters<NonNullable<typeof onWorkspaceMultiSelect>>[0],
        e,
      );
    }
  };

  const repoName = homeRepoPath
    ? homeRepoPath.split("/").filter(Boolean).pop() || "Repository"
    : "Repository";

  // GitHub/Linear/Settings are their own nav destinations — don't keep home/workspace
  // selection highlighted alongside them.
  const workspaceSelectionActive =
    currentPage !== "github" &&
    currentPage !== "linear" &&
    currentPage !== "trello" &&
    currentPage !== "jira" &&
    currentPage !== "settings" &&
    currentPage !== "artifacts";
  const isActiveRepo = (path: string | undefined) =>
    !isMultiRepo || path === repoPath;
  const isHomeSelected = (path: string | undefined) =>
    workspaceSelectionActive &&
    isActiveRepo(path) &&
    selectedWorkspaceId === null;

  // Only the repository on screen gets selection and workspace actions, so
  // stacks, multi-select, and drag-to-move never cross repositories. Rows in
  // other repositories switch to that repository when clicked.
  const renderWorkspaceList = (path: string | undefined, label?: string) => {
    const active = isActiveRepo(path);
    const selectionShown = active && workspaceSelectionActive;
    return (
      <RepoWorkspaceList
        key={path ?? ""}
        repoPath={path}
        cacheKey={active ? contextCacheKey : path}
        groupLabel={label}
        collapsed={label ? collapsedRepos.has(path ?? "") : false}
        onToggleCollapsed={() => toggleCollapsed(path ?? "")}
        activeSelectedWorkspaceId={
          selectionShown ? selectedWorkspaceId : undefined
        }
        activeSelectedWorkspaceIds={
          selectionShown ? selectedWorkspaceIds : undefined
        }
        selectedWorkspaceIds={active ? selectedWorkspaceIds : undefined}
        onWorkspaceClick={onWorkspaceClick}
        onWorkspaceMultiSelect={active ? onWorkspaceMultiSelect : undefined}
        onBulkArchive={onBulkArchive}
        onArchiveWorkspace={active ? onArchiveWorkspace : undefined}
        archivingWorkspaceIds={active ? archivingWorkspaceIds : undefined}
        exitingWorkspaceIds={active ? exitingWorkspaceIds : undefined}
        onAddAfter={active ? onAddAfter : undefined}
        onMoveWorkspace={active ? onMoveWorkspace : undefined}
        onSelectStack={active ? onSelectStack : undefined}
        onStartAgent={active ? onStartAgent : undefined}
        onStartShell={active ? onStartShell : undefined}
        terminalSessions={terminalSessions}
        onDropChangeFiles={active ? onDropChangeFiles : undefined}
      />
    );
  };

  const renderHomeRow = (
    path: string | undefined,
    options: { supporting?: SupportingRepo } = {},
  ) => {
    const active = isActiveRepo(path);
    const { supporting } = options;
    const missing = supporting ? !supporting.exists : false;
    return (
      <HomeRepoSidebarRow
        key={path ?? ""}
        repoPath={path}
        repoLabel={isMultiRepo ? labels.get(path ?? "") : undefined}
        missing={missing}
        homeRepoDisplayRef={active ? homeRepoDisplayRef : null}
        isHomeSelected={isHomeSelected(path)}
        selectedWorkspaceIds={active ? selectedWorkspaceIds : undefined}
        onWorkspaceClick={
          active
            ? onWorkspaceClick
            : () => {
                if (path && !missing) onSelectRepoHome?.(path);
              }
        }
        onWorkspaceMultiSelect={active ? onWorkspaceMultiSelect : undefined}
        onOpenBranchSwitcher={active ? onOpenBranchSwitcher : undefined}
        onDropChangeFiles={active ? onDropChangeFiles : undefined}
        onStartAgent={active ? onStartHomeAgent : undefined}
        onStartShell={active ? onStartHomeShell : undefined}
        onStack={active ? onStackHome : undefined}
        onAddRepository={supporting ? undefined : onAddRepository}
        onOpenRepoSettings={
          supporting && !missing && path
            ? () => onOpenRepoSettings?.(path)
            : undefined
        }
        onLocateRepo={
          supporting && missing && path
            ? () => onLocateSupportingRepo?.(path)
            : undefined
        }
        onRemoveRepo={
          supporting && path ? () => onRemoveSupportingRepo?.(path) : undefined
        }
      />
    );
  };

  return (
    <TooltipProvider delay={200} timeout={100}>
      <Sidebar
        collapsible="none"
        className="group/sidebar relative h-screen border-r border-border"
        data-testid="workspace-sidebar"
      >
        <SidebarHeader>
          <div className="flex min-w-0 items-center gap-2">
            <button
              type="button"
              data-testid="command-palette-trigger"
              onClick={onOpenCommandPalette}
              className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden px-3 py-1.5 rounded-lg border border-border bg-muted/50 hover:bg-muted text-muted-foreground transition-colors"
            >
              <Search className="w-4 h-4 shrink-0" />
              <span
                className="min-w-0 flex-1 truncate text-left"
                title={repoName}
              >
                {repoName}
              </span>
              <KbdGroup className="shrink-0">
                <Kbd>⌘ + K</Kbd>
              </KbdGroup>
            </button>
            <WorkspaceSidebarHeaderActions
              currentPage={currentPage}
              onOpenArtifacts={onOpenArtifacts}
              openSettings={openSettings}
            />
          </div>
        </SidebarHeader>

        <SidebarContent className="select-none" onClick={handleContainerClick}>
          <SidebarGroup className="py-0">
            <SidebarGroupContent>
              <SidebarMenu>
                {renderHomeRow(homeRepoPath)}
                {supportingRepos.map((repo) =>
                  renderHomeRow(repo.path, { supporting: repo }),
                )}

                {onOpenGitHub && (
                  <WorkspaceSidebarPanelButton
                    page="github"
                    currentPage={currentPage}
                    onClick={onOpenGitHub}
                    icon={Github}
                    label="Github"
                    testId="github-sidebar-item"
                  />
                )}

                {onOpenLinear && linearIntegration && (
                  <WorkspaceSidebarPanelButton
                    page="linear"
                    currentPage={currentPage}
                    onClick={onOpenLinear}
                    icon={ListTodo}
                    label="Linear"
                    testId="linear-sidebar-item"
                  />
                )}

                {onOpenTrello && (
                  <WorkspaceSidebarPanelButton
                    page="trello"
                    currentPage={currentPage}
                    onClick={onOpenTrello}
                    icon={TRACKER_ICONS.trello}
                    label="Trello"
                    testId="trello-sidebar-item"
                  />
                )}

                {onOpenJira && (
                  <WorkspaceSidebarPanelButton
                    page="jira"
                    currentPage={currentPage}
                    onClick={onOpenJira}
                    icon={TRACKER_ICONS.jira}
                    label="Jira"
                    testId="jira-sidebar-item"
                  />
                )}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>

          {isMultiRepo ? (
            <>
              {renderWorkspaceList(
                homeRepoPath,
                labels.get(homeRepoPath ?? ""),
              )}
              {presentSupportingRepos.map((repo) =>
                renderWorkspaceList(repo.path, labels.get(repo.path)),
              )}
            </>
          ) : (
            renderWorkspaceList(repoPath)
          )}
        </SidebarContent>
        <WorkspaceSidebarResizeHandle />
      </Sidebar>
    </TooltipProvider>
  );
};
