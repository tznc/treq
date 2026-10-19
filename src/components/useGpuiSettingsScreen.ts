import {
  type GpuiScreenAction,
  type GpuiSettingsTab,
  useGpuiScreen,
} from "../lib/gpui-screen";

type SettingsTabValue =
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

/**
 * Draws the Repository, Application and Feature Preview tabs in GPUI, which
 * loads and saves them through the backend itself and tells React what
 * changed (`useGpuiNativeEvents`). React keeps the tab and Close.
 */
export function useGpuiSettingsScreen(params: {
  currentTab: SettingsTabValue;
  setCurrentTab: (tab: SettingsTabValue) => void;
  repoPath: string;
  currentBranch: string | null;
  onClose: () => void;
}) {
  const { currentTab } = params;
  const view = GPUI_SETTINGS_TABS.has(currentTab)
    ? ({
        kind: "settings",
        tab: currentTab,
        repoPath: params.repoPath || null,
        currentBranch: params.currentBranch,
      } as const)
    : null;

  useGpuiScreen(view, (action: GpuiScreenAction) => {
    if (action.kind === "press" && action.id === "settings.close") {
      params.onClose();
    } else if (action.kind === "field" && action.field === "settings.tab") {
      params.setCurrentTab(action.value as SettingsTabValue);
    }
  });
}
