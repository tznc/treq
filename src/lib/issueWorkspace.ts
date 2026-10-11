import { githubOpenOrCreateWorkspaceFromIssue } from "./api";
import { linearOpenOrCreateWorkspaceFromIssue } from "./api-linear";
import type { IssueAttachment } from "./promptAttachments";

export type SubItemResult = {
  id: string;
  workspaceId: number;
  created: boolean;
};
export type SubItemFailure = { id: string; error: string };

export type IssueWorkspace = {
  workspaceId: number;
  /** Sub-item workspaces opened after the parent's. */
  subItemResults: SubItemResult[];
  subItemFailures: SubItemFailure[];
};

type OpenedWorkspace = { workspaceId: number; created: boolean };

// The backend opens or creates exactly one workspace per call. Kickoffs go
// through this chain, so only one runs at a time: a second kickoff waits
// until the first has opened its issue and every sub-item.
let queue: Promise<unknown> = Promise.resolve();

function oneAtATime<T>(task: () => Promise<T>): Promise<T> {
  const run = queue.then(task, task);
  queue = run.catch(() => undefined);
  return run;
}

type OneIssue = Pick<IssueAttachment, "source" | "id" | "title" | "url">;

async function openOne(
  repoPath: string,
  { source, id, title, url }: OneIssue,
): Promise<OpenedWorkspace> {
  switch (source) {
    case "github": {
      const result = await githubOpenOrCreateWorkspaceFromIssue(
        repoPath,
        Number(id),
        title,
        url,
      );
      return { workspaceId: result.workspace_id, created: result.created };
    }
    default: {
      // "linear", the only other source.
      const result = await linearOpenOrCreateWorkspaceFromIssue(repoPath, id);
      return { workspaceId: result.workspace_id, created: result.created };
    }
  }
}

/**
 * Opens the workspace an attached issue's session runs in, creating it when
 * needed, then does the same for each sub-item in turn when asked. A failed
 * sub-item is reported and skipped; a failed parent throws.
 */
export function openOrCreateIssueWorkspace(
  repoPath: string,
  issue: IssueAttachment,
): Promise<IssueWorkspace> {
  return oneAtATime(() => kickoff(repoPath, issue));
}

async function kickoff(
  repoPath: string,
  issue: IssueAttachment,
): Promise<IssueWorkspace> {
  const parent = await openOne(repoPath, issue);
  const subItemResults: SubItemResult[] = [];
  const subItemFailures: SubItemFailure[] = [];
  if (issue.includeSubItems) {
    for (const id of issue.subItemIds) {
      try {
        // eslint-disable-next-line no-await-in-loop -- one workspace at a time
        const opened = await openOne(repoPath, {
          source: issue.source,
          id,
          title: "",
          url: "",
        });
        subItemResults.push({ id, ...opened });
      } catch (error) {
        subItemFailures.push({
          id,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }
  }
  return { workspaceId: parent.workspaceId, subItemResults, subItemFailures };
}
