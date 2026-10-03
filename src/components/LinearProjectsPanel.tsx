import { Loader2, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import useSWR from "swr";
import {
  type LinearComment,
  type LinearDocument,
  type LinearProject,
  linearGetViewer,
  linearListDocumentComments,
  linearListProjectComments,
  linearListProjectDocuments,
  linearListProjects,
} from "../lib/api-linear";
import { Button } from "./ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { LinearFilterMenu } from "./LinearFilterMenu";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { LinearCommentedContent } from "./LinearCommentedContent";
import { cn } from "../lib/utils";
import {
  AGENT_REVIEW_TARGET_LINEAR_DOCUMENT,
  AGENT_REVIEW_TARGET_LINEAR_PROJECT,
} from "../lib/api-types-review";

type ProjectFilters = {
  state?: string;
  leadId?: string;
};

const EMPTY_PROJECT_FILTERS: ProjectFilters = {};

type ProjectStandardView = "all" | "my-projects" | "active" | "backlog";

const PROJECT_STANDARD_VIEWS: { value: ProjectStandardView; label: string }[] =
  [
    { value: "all", label: "All" },
    { value: "my-projects", label: "Mine" },
    { value: "active", label: "Active" },
    { value: "backlog", label: "Backlog" },
  ];

function applyProjectStandardView(
  projects: LinearProject[],
  view: ProjectStandardView,
  viewerId: string | undefined,
): LinearProject[] {
  switch (view) {
    case "active":
      return projects.filter((p) => p.state === "started");
    case "backlog":
      return projects.filter((p) => p.state === "planned");
    case "my-projects":
      return viewerId ? projects.filter((p) => p.lead?.id === viewerId) : [];
    case "all":
    default:
      return projects;
  }
}

function deriveProjectFilterOptions(projects: LinearProject[]) {
  const stateSet = new Set<string>();
  const leadMap = new Map<string, string>();

  projects.forEach((project) => {
    stateSet.add(project.state);
    if (project.lead) leadMap.set(project.lead.id, project.lead.name);
  });

  return {
    states: Array.from(stateSet).sort(),
    leads: Array.from(leadMap, ([id, name]) => ({ id, name })),
  };
}

function applyProjectFilters(
  projects: LinearProject[],
  filters: ProjectFilters,
): LinearProject[] {
  return projects.filter((project) => {
    if (filters.state && project.state !== filters.state) return false;
    if (filters.leadId && project.lead?.id !== filters.leadId) return false;
    return true;
  });
}

export const LinearProjectsSection: React.FC<{ repoPath: string }> = ({
  repoPath,
}) => {
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(
    null,
  );
  const [openDocument, setOpenDocument] = useState<LinearDocument | null>(null);
  const [standardView, setStandardView] = useState<ProjectStandardView>("all");
  const [filters, setFilters] = useState<ProjectFilters>(EMPTY_PROJECT_FILTERS);

  const {
    data: projects = [],
    isLoading,
    error,
    mutate: refetch,
  } = useSWR(
    repoPath ? ["linear-projects", repoPath] : null,
    async () => await linearListProjects(repoPath),
    { revalidateOnFocus: false },
  );

  const { data: viewer } = useSWR(
    repoPath ? ["linear-viewer", repoPath] : null,
    async () => await linearGetViewer(repoPath),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  );

  const viewFilteredProjects = useMemo(
    () => applyProjectStandardView(projects, standardView, viewer?.id),
    [projects, standardView, viewer?.id],
  );

  const filterOptions = useMemo(
    () => deriveProjectFilterOptions(viewFilteredProjects),
    [viewFilteredProjects],
  );

  const filteredProjects = useMemo(
    () => applyProjectFilters(viewFilteredProjects, filters),
    [viewFilteredProjects, filters],
  );

  const selectedProject =
    filteredProjects.find((p) => p.id === selectedProjectId) ??
    filteredProjects[0] ??
    null;

  const {
    data: documents = [],
    isLoading: isLoadingDocuments,
    mutate: refetchDocuments,
  } = useSWR(
    repoPath && selectedProject
      ? (["linear-project-documents", repoPath, selectedProject.id] as const)
      : null,
    async ([, path, projectId]) =>
      await linearListProjectDocuments(path, projectId),
    { revalidateOnFocus: false },
  );

  const {
    data: projectComments = [],
    isLoading: isLoadingProjectComments,
    error: projectCommentsError,
  } = useSWR(
    repoPath && selectedProject
      ? (["linear-project-comments", repoPath, selectedProject.id] as const)
      : null,
    async ([, path, projectId]) =>
      await linearListProjectComments(path, projectId),
    { revalidateOnFocus: false },
  );

  return (
    <div className="flex-1 flex min-h-0" data-testid="linear-projects-section">
      <div className="w-72 shrink-0 border-r border-border flex flex-col min-h-0">
        <div className="flex items-center justify-between px-4 py-2 shrink-0">
          <span className="text-sm font-medium text-muted-foreground">
            Projects
          </span>
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

        <div className="px-4 pb-2 shrink-0">
          <Tabs
            value={standardView}
            onValueChange={(v) => setStandardView(v as ProjectStandardView)}
          >
            <TabsList className="text-sm">
              {PROJECT_STANDARD_VIEWS.map((view) => (
                <TabsTrigger key={view.value} value={view.value}>
                  {view.label}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        </div>

        <div className="px-4 pb-2 shrink-0">
          <LinearFilterMenu
            testId="linear-projects-filter-trigger"
            groups={[
              {
                key: "status",
                label: "Status",
                value: filters.state,
                options: filterOptions.states.map((state) => ({
                  value: state,
                  label: state,
                })),
                onChange: (value) =>
                  setFilters((f) => ({ ...f, state: value })),
              },
              {
                key: "lead",
                label: "Lead",
                value: filters.leadId,
                options: filterOptions.leads.map((lead) => ({
                  value: lead.id,
                  label: lead.name,
                })),
                onChange: (value) =>
                  setFilters((f) => ({ ...f, leadId: value })),
              },
            ]}
          />
        </div>

        <div className="flex-1 overflow-y-auto">
          {isLoading && (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          )}

          {error !== undefined && error !== null && (
            <div className="p-4 text-sm text-destructive">
              {error instanceof Error
                ? error.message
                : "Failed to load projects"}
            </div>
          )}

          {!isLoading && !error && filteredProjects.length === 0 && (
            <div className="p-4 text-sm text-muted-foreground">
              No projects found
            </div>
          )}

          <div className="divide-y divide-border">
            {filteredProjects.map((project) => (
              <button
                key={project.id}
                type="button"
                data-testid={`linear-project-item-${project.id}`}
                className={cn(
                  "w-full text-left px-4 py-3 hover:bg-muted/50 transition-colors",
                  selectedProject?.id === project.id && "bg-muted",
                )}
                onClick={() => setSelectedProjectId(project.id)}
              >
                <p className="text-sm font-medium truncate">{project.name}</p>
                <div className="flex items-center gap-2 mt-1">
                  <span className="text-xs px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground capitalize">
                    {project.state}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {Math.round((project.progress || 0) * 100)}%
                  </span>
                </div>
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto">
        {selectedProject ? (
          <ProjectDetail
            project={selectedProject}
            documents={documents}
            isLoadingDocuments={isLoadingDocuments}
            onOpenDocument={setOpenDocument}
            comments={projectComments}
            isLoadingComments={isLoadingProjectComments}
            commentsError={projectCommentsError}
            onContentChanged={() => void refetch()}
          />
        ) : (
          <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
            Select a project
          </div>
        )}
      </div>

      <DocumentDialog
        repoPath={repoPath}
        // Read the latest copy so an applied suggestion shows up at once.
        document={
          documents.find((doc) => doc.id === openDocument?.id) ?? openDocument
        }
        onClose={() => setOpenDocument(null)}
        onContentChanged={() => void refetchDocuments()}
      />
    </div>
  );
};

const ProjectDetail: React.FC<{
  project: LinearProject;
  documents: LinearDocument[];
  isLoadingDocuments: boolean;
  onOpenDocument: (document: LinearDocument) => void;
  comments: LinearComment[];
  isLoadingComments: boolean;
  commentsError: unknown;
  onContentChanged: () => void;
}> = ({
  project,
  documents,
  isLoadingDocuments,
  onOpenDocument,
  comments,
  isLoadingComments,
  commentsError,
  onContentChanged,
}) => (
  <div className="p-4" data-testid="linear-project-detail">
    <div className="flex items-center gap-2 flex-wrap">
      <h2 className="text-lg font-semibold">{project.name}</h2>
      <a
        href={project.url}
        target="_blank"
        rel="noreferrer"
        className="text-xs text-primary hover:underline"
      >
        Open in Linear
      </a>
    </div>
    <div className="flex items-center gap-2 mt-2 text-sm text-muted-foreground flex-wrap">
      <span className="px-1.5 py-0.5 rounded-full bg-muted capitalize">
        {project.state}
      </span>
      <span>{Math.round((project.progress || 0) * 100)}% complete</span>
      {project.lead && <span>Lead: {project.lead.name}</span>}
      {project.target_date && <span>Target: {project.target_date}</span>}
    </div>

    <div className="mt-6">
      <h3 className="text-sm font-medium text-muted-foreground mb-2">
        Documents
      </h3>
      {isLoadingDocuments && (
        <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
      )}
      {!isLoadingDocuments && documents.length === 0 && (
        <p className="text-sm text-muted-foreground">No documents</p>
      )}
      <div className="flex flex-wrap gap-2">
        {documents.map((doc) => (
          <button
            key={doc.id}
            type="button"
            data-testid={`linear-document-item-${doc.id}`}
            className="text-sm px-3 py-1 rounded-full bg-muted hover:bg-muted/70 text-foreground transition-colors"
            onClick={() => onOpenDocument(doc)}
          >
            {doc.title}
          </button>
        ))}
      </div>
    </div>

    <div className="mt-6 pt-4 border-t border-border">
      <LinearCommentedContent
        content={project.description || "_No description._"}
        comments={comments}
        isLoadingComments={isLoadingComments}
        commentsError={commentsError}
        review={{
          targetType: AGENT_REVIEW_TARGET_LINEAR_PROJECT,
          targetId: project.id,
          title: project.name,
          url: project.url,
          body: project.description ?? "",
          onContentChanged,
        }}
      />
    </div>
  </div>
);

const DocumentDialog: React.FC<{
  repoPath: string;
  document: LinearDocument | null;
  onClose: () => void;
  onContentChanged: () => void;
}> = ({ repoPath, document: doc, onClose, onContentChanged }) => {
  const {
    data: comments = [],
    isLoading: isLoadingComments,
    error: commentsError,
  } = useSWR(
    repoPath && doc
      ? (["linear-document-comments", repoPath, doc.id] as const)
      : null,
    async ([, path, documentId]) =>
      await linearListDocumentComments(path, documentId),
    { revalidateOnFocus: false },
  );

  return (
    <Dialog open={doc !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-4xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{doc?.title}</DialogTitle>
        </DialogHeader>
        <LinearCommentedContent
          content={doc?.content || "No content"}
          comments={comments}
          isLoadingComments={isLoadingComments}
          commentsError={commentsError}
          review={
            doc
              ? {
                  targetType: AGENT_REVIEW_TARGET_LINEAR_DOCUMENT,
                  targetId: doc.id,
                  title: doc.title,
                  url: doc.url,
                  body: doc.content ?? "",
                  onContentChanged,
                }
              : undefined
          }
        />
      </DialogContent>
    </Dialog>
  );
};
