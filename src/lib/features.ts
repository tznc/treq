import pkg from "../../package.json";

export const FEATURES = pkg.featureFlags;

export const PREVIEW_FEATURE_IDS = [
  "skillsInstallation",
  "remoteSsh",
  "linearIntegration",
  "trelloIntegration",
  "jiraIntegration",
  "logs",
  "browser",
  "agentMessageQueue",
] as const;

export type PreviewFeatureId = (typeof PREVIEW_FEATURE_IDS)[number];

export type PreviewFeature = {
  id: PreviewFeatureId;
  title: string;
  docsPath: string;
};

export const PREVIEW_FEATURES: readonly PreviewFeature[] = [
  {
    id: "skillsInstallation",
    title: "Skills installation",
    docsPath: "/docs/how-to/installing-skills",
  },
  {
    id: "remoteSsh",
    title: "Remote SSH",
    docsPath: "/docs/how-to/remote-ssh-workspaces",
  },
  {
    id: "linearIntegration",
    title: "Linear integration",
    docsPath: "/docs/concepts/linear-integration",
  },
  {
    id: "trelloIntegration",
    title: "Trello integration",
    docsPath: "/docs/concepts/trello-and-jira-integrations",
  },
  {
    id: "jiraIntegration",
    title: "Jira integration",
    docsPath: "/docs/concepts/trello-and-jira-integrations",
  },
  {
    id: "logs",
    title: "Logs",
    docsPath: "/docs/concepts/logs",
  },
  {
    id: "browser",
    title: "Browser",
    docsPath: "/docs/concepts/browser",
  },
  {
    id: "agentMessageQueue",
    title: "Agent message queue",
    docsPath: "/docs/concepts/agent-sessions",
  },
];

export function previewSettingKey(id: PreviewFeatureId): string {
  return `feature_preview.${id}`;
}

/** Startup default: Vite dev (and Vitest) turn every preview flag on. */
export function previewFeatureDefault(id: PreviewFeatureId): boolean {
  if (import.meta.env.DEV || import.meta.env.MODE === "test") {
    return true;
  }
  return FEATURES[id] === true;
}

export function parsePreviewSetting(
  raw: string | null | undefined,
  id: PreviewFeatureId,
): boolean {
  if (raw === "true") return true;
  if (raw === "false") return false;
  return previewFeatureDefault(id);
}
