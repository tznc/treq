import { createSession, setRepoSetting } from "./api";
import type { AgentKind } from "./agents";
import {
  type IssueWorkspace,
  openOrCreateIssueWorkspace,
} from "./issueWorkspace";
import {
  formatPromptWithIssue,
  ISSUE_SOURCES,
  type IssueAttachment,
  type IssueSource,
} from "./promptAttachments";
import type { SessionCreationInfo } from "../types/sessions";

type Toast = {
  title: string;
  description: string;
  type: "success" | "warning";
};

/**
 * Sub-items open one workspace at a time after the parent, but only the
 * parent's workspace opens here, so tell the user what happened to the rest.
 */
function subItemToasts(
  source: IssueSource,
  { subItemResults, subItemFailures }: IssueWorkspace,
): Toast[] {
  const noun = ISSUE_SOURCES[source].subItemNoun.replace(/s$/, "");
  const label = noun.charAt(0).toUpperCase() + noun.slice(1);
  const toasts: Toast[] = [];
  if (subItemResults.length > 0) {
    const created = subItemResults.filter((r) => r.created).length;
    const opened = subItemResults.length - created;
    toasts.push({
      title: `${label} workspaces ready`,
      description:
        opened > 0
          ? `Created ${created} and opened ${opened} existing.`
          : `Created ${created}.`,
      type: "success",
    });
  }
  if (subItemFailures.length > 0) {
    const count = subItemFailures.length;
    toasts.push({
      title: `${count} ${noun} workspace${count === 1 ? "" : "s"} failed`,
      description: subItemFailures.map((f) => f.error).join("; "),
      type: "warning",
    });
  }
  return toasts;
}

/**
 * What `TaskInput` does on submit, for the agent prompt dialog GPUI draws:
 * saves the agent as the repository's default when asked, opens the attached
 * issue's workspace, and creates the session. Returns it for the caller to
 * start, or null when there is nothing to send. Throws when a step fails.
 */
export async function startAgentTask({
  repoPath,
  workspaceId,
  text,
  issue,
  agent,
  mode,
  saveAsRepoDefault,
  addToast,
}: {
  repoPath: string;
  workspaceId: number | null;
  text: string;
  issue: IssueAttachment | null;
  agent: AgentKind;
  mode: "plan" | "acceptEdits";
  saveAsRepoDefault: boolean;
  addToast: (toast: Toast) => void;
}): Promise<SessionCreationInfo | null> {
  const trimmed = text.trim();
  if (!trimmed && !issue) return null;

  const pendingPrompt = issue ? formatPromptWithIssue(trimmed, issue) : trimmed;
  if (saveAsRepoDefault) {
    await setRepoSetting(repoPath, "default_agent", agent);
  }

  const sessionName =
    pendingPrompt.length > 50
      ? `${pendingPrompt.slice(0, 47)}...`
      : pendingPrompt;

  const issueWorkspace = issue
    ? await openOrCreateIssueWorkspace(repoPath, issue)
    : null;
  const targetWorkspaceId = issueWorkspace?.workspaceId ?? workspaceId;
  if (issue && issueWorkspace) {
    subItemToasts(issue.source, issueWorkspace).forEach(addToast);
  }

  const sessionId = await createSession(
    repoPath,
    targetWorkspaceId,
    sessionName,
  );
  return {
    sessionId,
    sessionName,
    workspaceId: targetWorkspaceId,
    workspacePath: null,
    repoPath,
    pendingPrompt,
    permissionMode: mode,
    agent,
  };
}
