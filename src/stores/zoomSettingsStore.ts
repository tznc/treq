import { create } from "zustand";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { setSetting } from "../lib/api";

const DEFAULT_ZOOM = 100;
export const ZOOM_STEP = 5;
export const MIN_ZOOM = 50;
export const MAX_ZOOM = 200;

function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value));
}

export function applyZoomToDocument(zoom: number) {
  try {
    void getCurrentWebview()
      .setZoom(zoom / 100)
      .catch((error) => console.error("Failed to apply UI zoom", error));
  } catch {
    // Unit and integration tests run without Tauri's window metadata.
  }
}

export interface ZoomSettingsState {
  zoom: number;
  setZoom: (value: number) => Promise<void>;
  zoomIn: () => Promise<void>;
  zoomOut: () => Promise<void>;
  hydrateZoom: (zoom: number) => void;
}

export const defaultZoomSettingsState = {
  zoom: DEFAULT_ZOOM,
};

export const useZoomSettingsStore = create<ZoomSettingsState>((set, get) => ({
  ...defaultZoomSettingsState,
  hydrateZoom: (zoom) => set({ zoom: clampZoom(zoom) }),
  setZoom: async (value) => {
    const clamped = clampZoom(value);
    set({ zoom: clamped });
    applyZoomToDocument(clamped);
    await setSetting("ui_zoom", clamped.toString());
  },
  zoomIn: async () => {
    const next = clampZoom(get().zoom + ZOOM_STEP);
    set({ zoom: next });
    applyZoomToDocument(next);
    await setSetting("ui_zoom", next.toString());
  },
  zoomOut: async () => {
    const next = clampZoom(get().zoom - ZOOM_STEP);
    set({ zoom: next });
    applyZoomToDocument(next);
    await setSetting("ui_zoom", next.toString());
  },
}));
