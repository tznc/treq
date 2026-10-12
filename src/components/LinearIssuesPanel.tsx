import { Loader2, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import useSWR from "swr";
import {
  type LinearIssue,
  linearGetViewer,
  linearListIssues,
} from "../lib/api-linear";
import type { LinearIssueAttachment } from "../lib/promptAttachments";
import { Button } from "./ui/button";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { LinearFilterMenu } from "./LinearFilterMenu";
import {
  type KanbanColumn,
  LinearIssuesList,
  LinearKanbanView,
} from "./LinearIssueRows";
import { cn } from "../lib/utils";

type ViewMode = "list" | "kanban";
type StandardView = "all" | "active" | "my-issues" | "backlog";

const STANDARD_VIEWS: { value: StandardView; label: string }[] = [
  { value: "all", label: "All Issues" },
  { value: "active", label: "Active" },
  { value: "my-issues", label: "My Issues" },
  { value: "backlog", label: "Backlog" },
];

type IssueFilters = {
  assigneeId?: string;
  priority?: number;
  label?: string;
  projectId?: string;
};

const EMPTY_FILTERS: IssueFilters = {};

function applyStandardView(
  issues: LinearIssue[],
  view: StandardView,
  viewerId: string | undefined,
): LinearIssue[] {
  switch (view) {
    case "active":
      return issues.filter(
        (i) => i.state.type === "started" || i.state.type === "unstarted",
      );
    case "backlog":
      return issues.filter((i) => i.state.type === "backlog");
    case "my-issues":
      return viewerId ? issues.filter((i) => i.assignee?.id === viewerId) : [];
    case "all":
    default:
      return issues;
  }
}

function deriveFilterOptions(issues: LinearIssue[]) {
  const assigneeMap = new Map<string, string>();
  const priorityMap = new Map<number, string>();
  const labelSet = new Set<string>();
  const projectMap = new Map<string, string>();

  issues.forEach((issue) => {
    if (issue.assignee) assigneeMap.set(issue.assignee.id, issue.assignee.name);
    if (issue.priority !== undefined)
      priorityMap.set(
        issue.priority,
        issue.priority_label || String(issue.priority),
      );
    issue.labels.forEach((label) => labelSet.add(label));
    if (issue.project) projectMap.set(issue.project.id, issue.project.name);
  });

  return {
    assignees: Array.from(assigneeMap, ([id, name]) => ({ id, name })),
    priorities: Array.from(priorityMap, ([value, label]) => ({
      value,
      label,
    })).sort((a, b) => a.value - b.value),
    labels: Array.from(labelSet).sort(),
    projects: Array.from(projectMap, ([id, name]) => ({ id, name })),
  };
}

function applyIssueFilters(
  issues: LinearIssue[],
  filters: IssueFilters,
): LinearIssue[] {
  return issues.filter((issue) => {
    if (filters.assigneeId && issue.assignee?.id !== filters.assigneeId)
      return false;
    if (filters.priority !== undefined && issue.priority !== filters.priority)
      return false;
    if (filters.label && !issue.labels.includes(filters.label)) return false;
    if (filters.projectId && issue.project?.id !== filters.projectId)
      return false;
    return true;
  });
}

// A sub-issue nests under its parent only when the parent is also shown.
// Otherwise it is listed as a root so filters never hide it.
function splitRootIssues(issues: LinearIssue[]) {
  const shownIds = new Set(issues.map((i) => i.id));
  const rootIssues: LinearIssue[] = [];
  const subissuesMap = new Map<string, LinearIssue[]>();

  issues.forEach((issue) => {
    if (issue.parent_id && shownIds.has(issue.parent_id)) {
      const siblings = subissuesMap.get(issue.parent_id) ?? [];
      siblings.push(issue);
      subissuesMap.set(issue.parent_id, siblings);
    } else {
      rootIssues.push(issue);
    }
  });

  return { rootIssues, subissuesMap };
}

// Linear workflow order. Unknown state types sort last.
const STATE_TYPE_ORDER = [
  "triage",
  "backlog",
  "unstarted",
  "started",
  "completed",
  "canceled",
];

function stateTypeRank(type: string): number {
  const rank = STATE_TYPE_ORDER.indexOf(type);
  return rank === -1 ? STATE_TYPE_ORDER.length : rank;
}

function groupByState(issues: LinearIssue[]): KanbanColumn[] {
  const columns = new Map<string, KanbanColumn>();
  issues.forEach((issue) => {
    const column = columns.get(issue.state.name) ?? {
      name: issue.state.name,
      type: issue.state.type,
      issues: [],
    };
    column.issues.push(issue);
    columns.set(issue.state.name, column);
  });
  return Array.from(columns.values()).sort(
    (a, b) =>
      stateTypeRank(a.type) - stateTypeRank(b.type) ||
      a.name.localeCompare(b.name),
  );
}

export const LinearIssuesSection: React.FC<{
  repoPath: string;
  selectedTeam: string | undefined;
  onStartPromptFromIssue?: (issue: LinearIssueAttachment) => void;
}> = ({ repoPath, selectedTeam, onStartPromptFromIssue }) => {
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [standardView, setStandardView] = useState<StandardView>("all");
  const [filters, setFilters] = useState<IssueFilters>(EMPTY_FILTERS);

  const { data: viewer } = useSWR(
    repoPath ? ["linear-viewer", repoPath] : null,
    async () => await linearGetViewer(repoPath),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  );

  const {
    data: issues = [],
    isLoading,
    error,
    mutate: refetch,
  } = useSWR(
    repoPath ? ["linear-issues", repoPath, selectedTeam] : null,
    async () => await linearListIssues(repoPath, selectedTeam),
    { revalidateOnFocus: false },
  );

  // Use the issue's real sub-issues, not the ones the current filter shows,
  // to match what the backend kicks off.
  const handleKickoff = (issueId: string) => {
    const issue = issues.find((candidate) => candidate.id === issueId);
    if (issue)
      onStartPromptFromIssue?.({
        ...issue,
        includeSubissues: issue.sub_issue_ids.length > 0,
      });
  };

  const viewFilteredIssues = useMemo(
    () => applyStandardView(issues, standardView, viewer?.id),
    [issues, standardView, viewer?.id],
  );

  const filterOptions = useMemo(
    () => deriveFilterOptions(viewFilteredIssues),
    [viewFilteredIssues],
  );

  const filteredIssues = useMemo(
    () => applyIssueFilters(viewFilteredIssues, filters),
    [viewFilteredIssues, filters],
  );

  const { rootIssues, subissuesMap } = splitRootIssues(filteredIssues);
  const kanbanColumns = groupByState(filteredIssues);
  const identifiersById = new Map(issues.map((i) => [i.id, i.identifier]));

  return (
    <>
      <div className="px-4 pb-2 shrink-0">
        <Tabs
          value={standardView}
          onValueChange={(v) => setStandardView(v as StandardView)}
        >
          <TabsList className="text-base">
            {STANDARD_VIEWS.map((view) => (
              <TabsTrigger key={view.value} value={view.value}>
                {view.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </div>

      <div className="flex items-center gap-2 px-4 pb-2 shrink-0 flex-wrap">
        <Tabs
          value={viewMode}
          onValueChange={(v) => setViewMode(v as ViewMode)}
        >
          <TabsList className="text-base">
            <TabsTrigger value="list">List</TabsTrigger>
            <TabsTrigger value="kanban">Kanban</TabsTrigger>
          </TabsList>
        </Tabs>

        <LinearFilterMenu
          testId="linear-issues-filter-trigger"
          groups={[
            {
              key: "assignee",
              label: "Assignee",
              value: filters.assigneeId,
              options: filterOptions.assignees.map((a) => ({
                value: a.id,
                label: a.name,
              })),
              onChange: (value) =>
                setFilters((f) => ({ ...f, assigneeId: value })),
            },
            {
              key: "priority",
              label: "Priority",
              value:
                filters.priority !== undefined
                  ? String(filters.priority)
                  : undefined,
              options: filterOptions.priorities.map((p) => ({
                value: String(p.value),
                label: p.label,
              })),
              onChange: (value) =>
                setFilters((f) => ({
                  ...f,
                  priority: value !== undefined ? Number(value) : undefined,
                })),
            },
            {
              key: "label",
              label: "Label",
              value: filters.label,
              options: filterOptions.labels.map((l) => ({
                value: l,
                label: l,
              })),
              onChange: (value) => setFilters((f) => ({ ...f, label: value })),
            },
            {
              key: "project",
              label: "Project",
              value: filters.projectId,
              options: filterOptions.projects.map((p) => ({
                value: p.id,
                label: p.name,
              })),
              onChange: (value) =>
                setFilters((f) => ({ ...f, projectId: value })),
            },
          ]}
        />

        <div className="flex-1" />
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => void refetch()}
          title="Refresh"
          disabled={isLoading}
        >
          <RefreshCw
            className={cn("w-3.5 h-3.5", isLoading && "animate-spin")}
          />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto">
        {isLoading && (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
          </div>
        )}

        {error !== undefined && error !== null && (
          <div className="flex flex-col items-center justify-center h-full text-center p-8 gap-3">
            <p className="text-base text-destructive">
              {error instanceof Error ? error.message : "Failed to load issues"}
            </p>
          </div>
        )}

        {!isLoading && !error && filteredIssues.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-center p-8 gap-3">
            <p className="text-base text-muted-foreground">No issues found</p>
          </div>
        )}

        {!isLoading &&
          !error &&
          filteredIssues.length > 0 &&
          viewMode === "list" && (
            <LinearIssuesList
              repoPath={repoPath}
              issues={rootIssues}
              subissuesMap={subissuesMap}
              onKickoff={handleKickoff}
              onContentChanged={() => void refetch()}
            />
          )}

        {!isLoading &&
          !error &&
          filteredIssues.length > 0 &&
          viewMode === "kanban" && (
            <LinearKanbanView
              columns={kanbanColumns}
              identifiersById={identifiersById}
              onKickoff={handleKickoff}
            />
          )}
      </div>
    </>
  );
};
