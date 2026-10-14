import pkg from "../../package.json";

export const FEATURES = pkg.featureFlags;

export const PREVIEW_FEATURE_IDS = [
  "skillsInstallation",
  "remoteSsh",
  "linearIntegration",
  "logs",
  "browser",
  "devcontainers",
  "gpuiSidebar",
] as const;

export type PreviewFeatureId = (typeof PREVIEW_FEATURE_IDS)[number];

export type PreviewFeature = {
  id: PreviewFeatureId;
  title: string;
  docsPath: string;
  /** Changes how the window is built, so a switch only applies after a restart. */
  restartRequired?: boolean;
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
    id: "devcontainers",
    title: "Devcontainer environments",
    docsPath: "/docs/how-to/devcontainer-environments",
  },
  {
    id: "gpuiSidebar",
    title: "GPUI sidebar",
    docsPath: "/docs/concepts/gpui-sidebar",
    restartRequired: true,
  },
];

export function previewSettingKey(id: PreviewFeatureId): string {
  return `feature_preview.${id}`;
}

/** Startup default: Vite dev (and Vitest) turn every preview flag on, except `gpuiSidebar`. */
export function previewFeatureDefault(id: PreviewFeatureId): boolean {
  // The Rust side builds the window differently when this is on, so dev and
  // test builds do not turn it on by default.
  if (id === "gpuiSidebar") return FEATURES.gpuiSidebar === true;
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
