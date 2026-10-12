import { create } from "zustand";
import { setSetting } from "../lib/api";

export const DEFAULT_SIDEBAR_WIDTH = 280;
export const MIN_SIDEBAR_WIDTH = 200;
export const MAX_SIDEBAR_WIDTH = 480;

function clampSidebarWidth(value: number): number {
  return Math.min(
    MAX_SIDEBAR_WIDTH,
    Math.max(MIN_SIDEBAR_WIDTH, Math.round(value)),
  );
}

export interface SidebarWidthState {
  width: number;
  setWidth: (value: number) => void;
  persistWidth: () => Promise<void>;
  hydrateWidth: (value: number) => void;
}

export const defaultSidebarWidthState = {
  width: DEFAULT_SIDEBAR_WIDTH,
};

export const useSidebarWidthStore = create<SidebarWidthState>((set, get) => ({
  ...defaultSidebarWidthState,
  hydrateWidth: (value) => set({ width: clampSidebarWidth(value) }),
  setWidth: (value) => set({ width: clampSidebarWidth(value) }),
  persistWidth: async () => {
    await setSetting("workspace_sidebar_width", get().width.toString());
  },
}));
