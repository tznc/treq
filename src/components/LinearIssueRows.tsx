import { ChevronRight } from "lucide-react";
import { useState } from "react";
import useSWR from "swr";
import { type LinearIssue, linearListIssueComments } from "../lib/api-linear";
import { Button } from "./ui/button";
import { LinearCommentedContent } from "./LinearCommentedContent";
import { cn } from "../lib/utils";
import { AGENT_REVIEW_TARGET_LINEAR_ISSUE } from "../lib/api-types-review";

type KickoffHandler = (issueId: string) => void;

export const LinearIssuesList: React.FC<{
  repoPath: string;
  issues: LinearIssue[];
  subissuesMap: Map<string, LinearIssue[]>;
  onKickoff: KickoffHandler;
  /** Reloads the issues after an agent review suggestion lands in Linear. */
  onContentChanged?: () => void;
}> = ({ repoPath, issues, subissuesMap, onKickoff, onContentChanged }) => (
  <div className="divide-y divide-border">
    {issues.map((issue) => {
      const subissues = subissuesMap.get(issue.id) || [];
      return (
        <div key={issue.id}>
          <LinearIssueRow
            repoPath={repoPath}
            issue={issue}
            indent={false}
            onKickoff={onKickoff}
            onContentChanged={onContentChanged}
          />
          {subissues.map((subissue) => (
            <LinearIssueRow
              key={subissue.id}
              repoPath={repoPath}
              issue={subissue}
              indent
              onKickoff={onKickoff}
              onContentChanged={onContentChanged}
            />
          ))}
        </div>
      );
    })}
  </div>
);

const LinearIssueRow: React.FC<{
  repoPath: string;
  issue: LinearIssue;
  indent: boolean;
  onKickoff: KickoffHandler;
  onContentChanged?: () => void;
}> = ({ repoPath, issue, indent, onKickoff, onContentChanged }) => {
  const [expanded, setExpanded] = useState(false);

  const {
    data: comments = [],
    isLoading: isLoadingComments,
    error: commentsError,
  } = useSWR(
    expanded ? ["linear-issue-comments", repoPath, issue.id] : null,
    async ([, path, issueId]) => await linearListIssueComments(path, issueId),
    { revalidateOnFocus: false },
  );

  return (
    <div
      className={cn(
        "hover:bg-muted/50 transition-colors",
        indent && "ml-6 border-l border-muted-foreground/20",
      )}
    >
      <div className="flex items-start gap-2 pr-4">
        <button
          type="button"
          className="flex items-start gap-3 px-4 py-3 flex-1 min-w-0 text-left"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
        >
          <ChevronRight
            className={cn(
              "w-4 h-4 mt-1 shrink-0 text-muted-foreground transition-transform",
              expanded && "rotate-90",
            )}
          />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <a
                href={issue.url}
                target="_blank"
                rel="noreferrer"
                onClick={(e) => e.stopPropagation()}
                className="font-mono text-sm font-medium text-primary hover:underline"
              >
                {issue.identifier}
              </a>
              <span className="text-xs font-medium px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground">
                {issue.state.name}
              </span>
              {issue.priority_label && issue.priority !== 0 && (
                <span className="text-xs font-medium px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground">
                  {issue.priority_label}
                </span>
              )}
              {issue.project && (
                <span className="text-xs font-medium px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground">
                  {issue.project.name}
                </span>
              )}
              {issue.assignee && (
                <span className="text-xs text-muted-foreground">
                  {issue.assignee.name}
                </span>
              )}
            </div>
            <p className="text-base mt-1 truncate">{issue.title}</p>
            {issue.labels.length > 0 && (
              <div className="flex gap-1 mt-2 flex-wrap">
                {issue.labels.map((label) => (
                  <span
                    key={label}
                    className="text-xs px-1.5 py-0.5 rounded-full bg-primary/10 text-primary/80"
                  >
                    {label}
                  </span>
                ))}
              </div>
            )}
          </div>
        </button>
        <Button
          size="sm"
          variant="outline"
          className="mt-3 h-7 shrink-0 text-xs"
          onClick={() => onKickoff(issue.id)}
        >
          Kick off
        </Button>
      </div>

      {expanded && (
        <div className="px-4 pb-4 pl-11" data-testid="linear-issue-expanded">
          <LinearCommentedContent
            content={issue.description || "_No description._"}
            comments={comments}
            isLoadingComments={isLoadingComments}
            commentsError={commentsError}
            review={{
              targetType: AGENT_REVIEW_TARGET_LINEAR_ISSUE,
              targetId: issue.id,
              title: `${issue.identifier} ${issue.title}`,
              url: issue.url,
              body: issue.description ?? "",
              onContentChanged,
            }}
          />
        </div>
      )}
    </div>
  );
};

export type KanbanColumn = {
  name: string;
  type: string;
  issues: LinearIssue[];
};

// Each issue, sub-issues included, sits in the column of its own state.
export const LinearKanbanView: React.FC<{
  columns: KanbanColumn[];
  identifiersById: Map<string, string>;
  onKickoff: KickoffHandler;
}> = ({ columns, identifiersById, onKickoff }) => (
  <div className="flex gap-3 overflow-x-auto p-4 h-full">
    {columns.map((column) => (
      <section
        key={column.name}
        aria-label={column.name}
        className="flex-shrink-0 w-80 bg-muted/30 rounded-lg border border-border p-3 flex flex-col"
      >
        <h3 className="font-medium text-sm mb-3 text-muted-foreground">
          {column.name}
        </h3>
        <div className="flex-1 overflow-y-auto flex flex-col gap-2">
          {column.issues.map((issue) => (
            <LinearKanbanCard
              key={issue.id}
              issue={issue}
              parentIdentifier={
                issue.parent_id
                  ? identifiersById.get(issue.parent_id)
                  : undefined
              }
              onKickoff={onKickoff}
            />
          ))}
        </div>
      </section>
    ))}
  </div>
);

const LinearKanbanCard: React.FC<{
  issue: LinearIssue;
  parentIdentifier: string | undefined;
  onKickoff: KickoffHandler;
}> = ({ issue, parentIdentifier, onKickoff }) => (
  <div className="bg-background border border-border rounded-md p-2.5 text-sm">
    <a
      href={issue.url}
      target="_blank"
      rel="noreferrer"
      className="font-mono text-xs font-medium text-primary hover:underline"
    >
      {issue.identifier}
    </a>
    <p className="text-xs font-medium mt-1 line-clamp-2">{issue.title}</p>
    {parentIdentifier && (
      <p className="text-xs text-muted-foreground mt-1">
        Sub-issue of {parentIdentifier}
      </p>
    )}
    {issue.labels.length > 0 && (
      <div className="flex gap-1 mt-1.5 flex-wrap">
        {issue.labels.slice(0, 2).map((label) => (
          <span
            key={label}
            className="text-xs px-1 py-0.5 rounded bg-primary/10 text-primary/70"
          >
            {label}
          </span>
        ))}
        {issue.labels.length > 2 && (
          <span className="text-xs px-1 py-0.5 text-muted-foreground">
            +{issue.labels.length - 2}
          </span>
        )}
      </div>
    )}
    <Button
      size="sm"
      variant="outline"
      className="w-full mt-2 text-xs h-7"
      onClick={() => onKickoff(issue.id)}
    >
      Kick off
    </Button>
  </div>
);
