import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef } from "react";
import { useThemeStore } from "../stores/themeStore";
import { gpuiScreenUpdate } from "./api";
import { isGpuiShellHost } from "./gpui-shell";
import { useWindowModalBackdrop } from "./window-modals";

/**
 * Screens GPUI draws in place of the webview (src-tauri/src/gpui_shell/screen).
 * React keeps rendering its own version underneath: GPUI hides the webview
 * while a screen is pushed, and shows it again for null.
 */

export interface GpuiChecklistTool {
  binary: string;
  label: string;
  note: string | null;
  required: boolean;
  /** null while the check is still running. */
  installed: boolean | null;
  installUrl: string;
}

export interface GpuiChecklist {
  title: string;
  validating: boolean;
  groups: { label: string; error: boolean; tools: GpuiChecklistTool[] }[];
}

/** One `<option>` of a `<select>`. */
export interface GpuiSelectOption {
  value: string;
  label: string;
}

export type GpuiSettingsTab =
  | "repository"
  | "application"
  | "account"
  | "integrations"
  | "skills"
  | "preview";

export interface GpuiApplicationSettings {
  theme: string;
  themeOptions: GpuiSelectOption[];
  zoom: number;
  zoomMin: number;
  zoomMax: number;
  zoomStep: number;
  fontSize: string;
  fontSizeOptions: GpuiSelectOption[];
  defaultModel: string;
  modelOptions: GpuiSelectOption[];
  defaultAgent: string;
  agentOptions: GpuiSelectOption[];
  conflictMarkerStyle: string;
  conflictOptions: GpuiSelectOption[];
  notifyAgentFinished: boolean;
  checklist: GpuiChecklist;
  setupScript: {
    script: string;
    alwaysRun: boolean;
    /** Status not loaded yet: the editor is disabled. */
    loading: boolean;
    canRun: boolean;
    /** The line beside Run: the status, or why it cannot run. */
    status: string;
    /** `status` is a load error, drawn in the destructive color. */
    statusError: boolean;
  };
}

export interface GpuiRepositorySettings {
  /** The `.treq/config.yaml` card, or null when it is not shown. */
  yamlCard: { error: string | null; loading: boolean } | null;
  currentBranch: string | null;
  /** "Loading settings..." in place of the fields. */
  loading: boolean;
  fields: {
    branchNamePattern: string;
    includedFiles: string;
    defaultModel: string;
    defaultAgent: string;
    autoPush: boolean;
    ignoreGeneratedAgentFiles: boolean;
    reviewPrompt: string;
    reviewAgent: string;
    autoReviewTrigger: string;
  };
  /** Fields `.treq/config.yaml` sets, drawn disabled. */
  managed: string[];
  modelOptions: GpuiSelectOption[];
  agentOptions: GpuiSelectOption[];
  reviewAgentOptions: GpuiSelectOption[];
  triggerOptions: GpuiSelectOption[];
  error: string | null;
}

export interface GpuiPreviewSettings {
  features: { id: string; title: string; checked: boolean }[];
}

export interface GpuiSettingsScreen {
  kind: "settings";
  tab: GpuiSettingsTab;
  /** Visible tabs, in order. */
  tabs: GpuiSettingsTab[];
  /** The header's save button, or null without one. */
  save: { label: string; disabled: boolean } | null;
  application: GpuiApplicationSettings | null;
  /** null without a repository: "Set repository in Application tab". */
  repository: GpuiRepositorySettings | null;
  preview: GpuiPreviewSettings | null;
}

export type GpuiScreenView =
  | {
      kind: "onboarding";
      remoteLabel: string | null;
      checklist: GpuiChecklist;
    }
  | GpuiSettingsScreen;

export type GpuiScreenSnapshot = GpuiScreenView & { dark: boolean };

export type GpuiScreenAction =
  | { kind: "openRepo" }
  | { kind: "openRemote" }
  | { kind: "recheckPrerequisites" }
  | { kind: "openUrl"; url: string }
  /** A control's new value; `field` names it, e.g. "app.theme". */
  | { kind: "field"; field: string; value: string | number | boolean }
  /** A slider released at `value`. */
  | { kind: "commit"; field: string; value: number }
  /** A button; `id` names it, e.g. "settings.save". */
  | { kind: "press"; id: string };

const GPUI_SCREEN_ACTION_EVENT = "gpui-screen-action";

/** Push a screen to GPUI; null shows the webview. Failures are logged, not thrown. */
export async function updateGpuiScreen(
  snapshot: GpuiScreenSnapshot | null,
): Promise<void> {
  try {
    await gpuiScreenUpdate(snapshot);
  } catch (error) {
    console.error("gpui_screen_update failed:", error);
  }
}

/** Listen for GPUI screen clicks sent to this webview window only. */
export function listenGpuiScreenActions(
  handler: (action: GpuiScreenAction) => void,
): Promise<() => void> {
  return getCurrentWebviewWindow().listen<GpuiScreenAction>(
    GPUI_SCREEN_ACTION_EVENT,
    (event) => handler(event.payload),
  );
}

/**
 * Draws `view` in GPUI while the calling component is mounted, and runs
 * `onAction` for its clicks. A webview modal (a dialog GPUI does not draw
 * yet) brings the webview back while it is open, so it stays usable.
 */
export function useGpuiScreen(
  view: GpuiScreenView | null,
  onAction: (action: GpuiScreenAction) => void,
): void {
  const host = isGpuiShellHost();
  const dark = useThemeStore((s) => s.actualTheme()) === "dark";
  const modal = useWindowModalBackdrop(host) !== null;
  const json =
    host && view && !modal ? JSON.stringify({ ...view, dark }) : null;

  const lastSent = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    if (!host || json === lastSent.current) return;
    lastSent.current = json;
    void updateGpuiScreen(json === null ? null : JSON.parse(json));
  }, [host, json]);

  useEffect(() => {
    if (!host) return;
    return () => {
      lastSent.current = undefined;
      void updateGpuiScreen(null);
    };
  }, [host]);

  const latest = useRef(onAction);
  latest.current = onAction;
  useEffect(() => {
    if (!host) return;
    const unlisten = listenGpuiScreenActions((action) =>
      latest.current(action),
    );
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [host]);
}
