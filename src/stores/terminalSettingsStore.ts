import { create } from "zustand";
import { setSetting } from "../lib/api";

const DEFAULT_TERMINAL_FONT_SIZE = 14;
export const MIN_TERMINAL_FONT_SIZE = 8;
export const MAX_TERMINAL_FONT_SIZE = 32;

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
