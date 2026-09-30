import { isAgentKind, type AgentKind } from "./agents";
import type { Workspace } from "./api";

export type AgentPermissionMode = "plan" | "acceptEdits";

export type AgentDeepLinkRequest = {
  repo: string;
  branch: string;
  prompt: string;
  mode: AgentPermissionMode;
  agent: AgentKind;
  requestId: string;
};

const PROCESSED_PREFIX = "treq.agent.processed.";
const PENDING_PREFIX = "treq.agent.pending.";
const CLAIM_PREFIX = "treq.agent.claim.";

const normalizeMode = (value: string): AgentPermissionMode | null => {
  if (value === "plan") return "plan";
  if (value === "acceptEdits" || value === "edit") return "acceptEdits";
  return null;
};

export const parseAgentDeepLinkUrl = (
  url: string,
): AgentDeepLinkRequest | null => {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }

  if (
    parsed.protocol !== "treq:" ||
    parsed.hostname !== "agent" ||
    parsed.pathname !== "/start"
  ) {
    return null;
  }

  const repo = parsed.searchParams.get("repo");
  const branch = parsed.searchParams.get("branch");
  const prompt = parsed.searchParams.get("prompt");
  const modeRaw = parsed.searchParams.get("mode");
  const agentRaw = parsed.searchParams.get("agent");
  const requestId = parsed.searchParams.get("request_id");
  if (!repo || !branch || !prompt || !modeRaw || !agentRaw || !requestId) {
    return null;
  }
  const mode = normalizeMode(modeRaw);
  if (!mode || !isAgentKind(agentRaw)) {
    return null;
  }

  return {
    repo,
    branch,
    prompt,
    mode,
    agent: agentRaw,
    requestId,
  };
};

export const parseAgentDeepLinks = (urls: string[]): AgentDeepLinkRequest[] =>
  urls
    .map((url) => parseAgentDeepLinkUrl(url))
    .filter((item): item is AgentDeepLinkRequest => item !== null);

export const isProcessedAgentRequest = (requestId: string): boolean =>
  localStorage.getItem(`${PROCESSED_PREFIX}${requestId}`) === "1";

export const markProcessedAgentRequest = (requestId: string): void => {
  localStorage.setItem(`${PROCESSED_PREFIX}${requestId}`, "1");
};

export const tryClaimAgentRequest = (requestId: string): boolean => {
  const key = `${CLAIM_PREFIX}${requestId}`;
  if (localStorage.getItem(key)) return false;
  const token = `${Date.now()}-${Math.random()}`;
  localStorage.setItem(key, token);
  return localStorage.getItem(key) === token;
};

export const queuePendingAgentRequest = (
  request: AgentDeepLinkRequest,
): void => {
  const key = `${PENDING_PREFIX}${request.repo}`;
  const existing = localStorage.getItem(key);
  const parsed: AgentDeepLinkRequest[] = existing ? JSON.parse(existing) : [];
  if (parsed.some((item) => item.requestId === request.requestId)) return;
  parsed.push(request);
  localStorage.setItem(key, JSON.stringify(parsed));
};

export const popPendingAgentRequests = (
  repo: string,
): AgentDeepLinkRequest[] => {
  const key = `${PENDING_PREFIX}${repo}`;
  const existing = localStorage.getItem(key);
  if (!existing) return [];

  let parsed: AgentDeepLinkRequest[] = [];
  try {
    parsed = JSON.parse(existing) as AgentDeepLinkRequest[];
  } catch {
    localStorage.removeItem(key);
    return [];
  }

  localStorage.removeItem(key);
  return parsed;
};

export const findWorkspaceByBranch = (
  workspaces: Workspace[],
  branch: string,
): Workspace | null =>
  workspaces.find((workspace) => workspace.branch_name === branch) ?? null;

export async function processAgentDeepLinkRequests(
  requests: AgentDeepLinkRequest[],
  context: {
    repoPath: string | null;
    /** Supporting repositories shown in this window; handled here too. */
    supportingRepoPaths?: string[];
    workspacesLength: number;
    onSameRepoRequest: (request: AgentDeepLinkRequest) => Promise<void>;
    deferRequest: (request: AgentDeepLinkRequest) => void;
    openOtherRepoWindow: (request: AgentDeepLinkRequest) => void;
  },
): Promise<void> {
  await requests.reduce<Promise<void>>(async (chain, request) => {
    await chain;
    if (isProcessedAgentRequest(request.requestId)) return;

    if (
      request.repo === context.repoPath ||
      context.supportingRepoPaths?.includes(request.repo)
    ) {
      if (!tryClaimAgentRequest(request.requestId)) return;
      // Supporting-repository requests look up their own workspaces.
      if (request.repo === context.repoPath && context.workspacesLength === 0) {
        context.deferRequest(request);
        return;
      }
      await context.onSameRepoRequest(request);
      return;
    }

    if (!tryClaimAgentRequest(request.requestId)) return;
    queuePendingAgentRequest(request);
    context.openOtherRepoWindow(request);
  }, Promise.resolve());
}
