interface AgentInfo {
  id: string;
  label: string;
  /** Has a plan mode, offered as "Plan" next to Edit. */
  hasPlanMode: boolean;
}

/**
 * Every agent treq can launch in a terminal, in the order pickers list them.
 * Agent pickers, settings and stored-value checks all read this list, so a
 * new agent is added here (and its launch command in agentCommand.ts).
 */
export const AGENTS = [
  { id: "claude", label: "Claude", hasPlanMode: true },
  { id: "codex", label: "Codex", hasPlanMode: false },
  { id: "cursor", label: "Cursor", hasPlanMode: true },
  {
    id: "copilot",
    label: "Copilot",
    hasPlanMode: false,
  },
] as const satisfies readonly AgentInfo[];

export type AgentKind = (typeof AGENTS)[number]["id"];

export const DEFAULT_AGENT: AgentKind = "claude";

export const isAgentKind = (value: unknown): value is AgentKind =>
  AGENTS.some((agent) => agent.id === value);

/** Narrows a stored agent setting, dropping anything unrecognized. */
export const toAgentKind = (
  value: string | null | undefined,
): AgentKind | undefined => (isAgentKind(value) ? value : undefined);

export const agentInfo = (agent: AgentKind): AgentInfo =>
  AGENTS.find((info) => info.id === agent) ?? AGENTS[0];
