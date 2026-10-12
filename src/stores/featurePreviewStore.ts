import { create } from "zustand";
import { setSetting } from "../lib/api";
import {
  FEATURES,
  PREVIEW_FEATURE_IDS,
  type PreviewFeatureId,
  parsePreviewSetting,
  previewSettingKey,
} from "../lib/features";

function previewFeatureDefault(id: PreviewFeatureId): boolean {
  if (import.meta.env.DEV || import.meta.env.MODE === "test") {
    return true;
  }
  return FEATURES[id] === true;
}

type PreviewFlags = Record<PreviewFeatureId, boolean>;

function defaultFlags(): PreviewFlags {
  return {
    skillsInstallation: previewFeatureDefault("skillsInstallation"),
    workspaceScheduling: previewFeatureDefault("workspaceScheduling"),
    remoteSsh: previewFeatureDefault("remoteSsh"),
    linearIntegration: previewFeatureDefault("linearIntegration"),
    trelloIntegration: previewFeatureDefault("trelloIntegration"),
    jiraIntegration: previewFeatureDefault("jiraIntegration"),
    logs: previewFeatureDefault("logs"),
    browser: previewFeatureDefault("browser"),
    agentMessageQueue: previewFeatureDefault("agentMessageQueue"),
  };
}

export interface FeaturePreviewState {
  flags: PreviewFlags;
  setPreviewFeature: (id: PreviewFeatureId, enabled: boolean) => Promise<void>;
  hydrateFlags: (settings: Record<string, string | null | undefined>) => void;
}

export const defaultFeaturePreviewState = {
  flags: defaultFlags(),
};

export const useFeaturePreviewStore = create<FeaturePreviewState>((set) => ({
  ...defaultFeaturePreviewState,
  hydrateFlags: (settings) => {
    const flags = { ...defaultFlags() };
    for (const id of PREVIEW_FEATURE_IDS) {
      flags[id] = parsePreviewSetting(settings[previewSettingKey(id)], id);
    }
    set({ flags });
  },
  setPreviewFeature: async (id, enabled) => {
    set((state) => ({
      flags: { ...state.flags, [id]: enabled },
    }));
    await setSetting(previewSettingKey(id), enabled ? "true" : "false");
  },
}));

export function usePreviewFeature(id: PreviewFeatureId): boolean {
  return useFeaturePreviewStore((state) => state.flags[id]);
}
