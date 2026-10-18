import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { AGENTS } from "../lib/agents";
import type { PreviewFeatureId } from "../lib/features";
import type { useGpuiPartsHost } from "../lib/gpui-parts";
import {
  type GpuiPreviewSettings,
  type GpuiRepositorySettings,
  type GpuiScreenAction,
  type GpuiSettingsTab,
  useGpuiScreen,
} from "../lib/gpui-screen";
import { FONT_SIZE_PRESETS } from "../stores/terminalSettingsStore";
import { MAX_ZOOM, MIN_ZOOM, ZOOM_STEP } from "../stores/zoomSettingsStore";
import { setupScriptRunState } from "./AppSetupScriptSettings";
import { useGpuiChecklist } from "./PrerequisiteChecklist";
import { CLAUDE_MODEL_OPTIONS } from "./RepositorySettingsContent";
import type { useAppSetupScript } from "./useAppSetupScript";

export type SettingsTabValue =
  | "application"
  | "repository"
  | "account"
  | "integrations"
  | "skills"
  | "preview";

const GPUI_SETTINGS_TABS: ReadonlySet<GpuiSettingsTab> = new Set([
  "repository",
  "application",
  "preview",
]);

interface GpuiApplicationState {
  theme: string;
  setTheme: (value: string) => void;
  zoom: number;
  setZoomDraft: (value: number) => void;
  commitZoom: (value: number) => void;
  fontSize: number;
  setFontSize: (value: number) => void;
  defaultModel: string;
  setDefaultModel: (value: string) => void;
  defaultAgent: string;
  setDefaultAgent: (value: string) => void;
  conflictMarkerStyle: string;
  setConflictMarkerStyle: (value: string) => void;
  notifyAgentFinished: boolean;
  setNotifyAgentFinished: (value: boolean) => void;
  setupScript: ReturnType<typeof useAppSetupScript>;
}

/**
 * Draws the Repository, Application and Feature Preview tabs in GPUI with
 * the same state and handlers as the React page.
 */
export function useGpuiSettingsScreen(params: {
  currentTab: SettingsTabValue;
  setCurrentTab: (tab: SettingsTabValue) => void;
  skillsInstallation: boolean;
  repoPath: string;
  currentBranch: string | null;
  savingRepository: boolean;
  parts: ReturnType<typeof useGpuiPartsHost>;
  application: GpuiApplicationState;
  onClose: () => void;
  onSaveApplication: () => Promise<void>;
  onSaveRepository: () => Promise<void>;
}) {
  const { currentTab, repoPath, parts, application: app } = params;
  const { checklist, recheck } = useGpuiChecklist("Command-line tools");
  // Set from a Run click until the status reports the run, as the React
  // Run button does, so a second click cannot start another run.
  const [starting, setStarting] = useState(false);
  const run = setupScriptRunState({
    script: app.setupScript.script,
    status: app.setupScript.status,
    starting,
    loadError: app.setupScript.loadError,
  });
  const agentOptions = AGENTS.map((a) => ({ value: a.id, label: a.label }));
  const repository = parts.parts.repository as
    | Omit<GpuiRepositorySettings, "yamlCard" | "currentBranch">
    | undefined;
  const yamlCard = parts.parts.yamlCard as
    | GpuiRepositorySettings["yamlCard"]
    | undefined;
  const preview = parts.parts.preview as GpuiPreviewSettings | undefined;

  const tabs: GpuiSettingsTab[] = [
    "repository",
    "application",
    "account",
    "integrations",
    ...(params.skillsInstallation ? (["skills"] as const) : []),
    "preview",
  ];
  const save =
    currentTab === "application"
      ? { label: "Save Settings", disabled: false }
      : currentTab === "repository" && repoPath
        ? {
            label: params.savingRepository ? "Saving..." : "Save Settings",
            disabled: params.savingRepository,
          }
        : null;

  const view = GPUI_SETTINGS_TABS.has(currentTab)
    ? ({
        kind: "settings",
        tab: currentTab,
        tabs,
        save,
        application:
          currentTab === "application"
            ? {
                theme: app.theme,
                themeOptions: [
                  { value: "system", label: "System" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ],
                zoom: app.zoom,
                zoomMin: MIN_ZOOM,
                zoomMax: MAX_ZOOM,
                zoomStep: ZOOM_STEP,
                fontSize: String(app.fontSize),
                fontSizeOptions: FONT_SIZE_PRESETS.map((preset) => ({
                  value: String(preset.size),
                  label: preset.label,
                })),
                defaultModel: app.defaultModel,
                modelOptions: [
                  { value: "", label: "Default" },
                  ...CLAUDE_MODEL_OPTIONS,
                ],
                defaultAgent: app.defaultAgent,
                agentOptions: [
                  { value: "", label: "Default (Claude)" },
                  ...agentOptions,
                ],
                conflictMarkerStyle: app.conflictMarkerStyle,
                conflictOptions: [
                  { value: "git", label: "Git Diff3 (Default)" },
                  { value: "diff", label: "JJ Diff" },
                  { value: "snapshot", label: "JJ Snapshot" },
                ],
                notifyAgentFinished: app.notifyAgentFinished,
                checklist,
                setupScript: {
                  script: app.setupScript.script,
                  alwaysRun: app.setupScript.alwaysRun,
                  loading: !app.setupScript.status,
                  canRun: run.canRun,
                  status: run.text,
                  statusError: run.error,
                },
              }
            : null,
        repository:
          currentTab === "repository" && repoPath && repository
            ? {
                ...repository,
                yamlCard: yamlCard ?? null,
                currentBranch: params.currentBranch,
              }
            : null,
        preview: currentTab === "preview" ? (preview ?? null) : null,
      } as const)
    : null;
  // The Repository tab waits for its fields before GPUI takes over.
  const ready =
    view !== null &&
    (currentTab !== "repository" || !repoPath || repository !== undefined);

  const press = (pressed: string) => {
    const [id, arg] = pressed.split(":");
    switch (id) {
      case "settings.close":
        params.onClose();
        break;
      case "settings.save":
        void (currentTab === "application"
          ? params.onSaveApplication()
          : params.onSaveRepository());
        break;
      case "app.setupScript.run":
        if (!run.canRun) break;
        setStarting(true);
        void app.setupScript.run().finally(() => setStarting(false));
        break;
      case "repo.yaml.reload":
        parts.handlers("yamlCard")?.reload?.();
        break;
      case "preview.docs":
        parts.handlers("preview")?.openDocs?.(arg as never);
        break;
      default:
        break;
    }
  };

  const setField = (field: string, value: unknown) => {
    if (field === "settings.tab") {
      params.setCurrentTab(value as SettingsTabValue);
    } else if (field.startsWith("repo.")) {
      parts
        .handlers("repository")
        ?.setField?.(field.slice(5) as never, value as never);
    } else if (field.startsWith("preview.")) {
      parts
        .handlers("preview")
        ?.toggle?.(field.slice(8) as PreviewFeatureId as never, value as never);
    } else {
      const setters: Record<string, (v: never) => void> = {
        "app.theme": app.setTheme,
        "app.zoom": app.setZoomDraft,
        "app.fontSize": (v: string) => app.setFontSize(parseInt(v, 10)),
        "app.defaultModel": app.setDefaultModel,
        "app.defaultAgent": app.setDefaultAgent,
        "app.conflictMarkerStyle": app.setConflictMarkerStyle,
        "app.notifyAgentFinished": app.setNotifyAgentFinished,
        "app.setupScript": app.setupScript.setScript,
        "app.setupScriptAlwaysRun": app.setupScript.setAlwaysRun,
      };
      setters[field]?.(value as never);
    }
  };

  useGpuiScreen(ready ? view : null, (action: GpuiScreenAction) => {
    switch (action.kind) {
      case "press":
        press(action.id);
        break;
      case "commit":
        if (action.field === "app.zoom") app.commitZoom(action.value);
        break;
      case "field":
        setField(action.field, action.value);
        break;
      case "recheckPrerequisites":
        recheck();
        break;
      case "openUrl":
        void openUrl(action.url);
        break;
      default:
        break;
    }
  });
}
