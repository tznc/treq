import { create } from "zustand";
import { setSetting } from "../lib/api";

const DEFAULT_TERMINAL_FONT_SIZE = 14;
export const MIN_TERMINAL_FONT_SIZE = 8;
export const MAX_TERMINAL_FONT_SIZE = 32;

export const FONT_SIZE_PRESETS = [
  { label: "Extra Small", size: 10 },
  { label: "Small", size: 12 },
  { label: "Medium (default)", size: DEFAULT_TERMINAL_FONT_SIZE },
  { label: "Large", size: 16 },
  { label: "Extra Large", size: 18 },
] as const;

/** Snaps a stored pixel size (older builds allowed any 8-32) to a preset. */
export function nearestFontSizePreset(size: number): number {
  return FONT_SIZE_PRESETS.reduce((best, p) =>
    Math.abs(p.size - size) < Math.abs(best.size - size) ? p : best,
  ).size;
}

export interface TerminalSettingsState {
  fontSize: number;
  setFontSize: (size: number) => Promise<void>;
  hydrateFontSize: (size: number) => void;
}

export const defaultTerminalSettingsState = {
  fontSize: DEFAULT_TERMINAL_FONT_SIZE,
};

function assertTerminalFontSize(size: number) {
  if (
    size < MIN_TERMINAL_FONT_SIZE ||
    size > MAX_TERMINAL_FONT_SIZE ||
    Number.isNaN(size)
  ) {
    throw new Error("Font size must be between 8 and 32");
  }
}

export const useTerminalSettingsStore = create<TerminalSettingsState>(
  (set) => ({
    ...defaultTerminalSettingsState,
    hydrateFontSize: (fontSize) => set({ fontSize }),
    setFontSize: async (size) => {
      assertTerminalFontSize(size);
      set({ fontSize: size });
      await setSetting("terminal_font_size", size.toString());
    },
  }),
);
