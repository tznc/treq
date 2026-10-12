import { create } from "zustand";
import { setSetting } from "../lib/api";

const DEFAULT_DIFF_FONT_SIZE = 11;
export const MIN_DIFF_FONT_SIZE = 8;
export const MAX_DIFF_FONT_SIZE = 16;

export interface DiffSettingsState {
  fontSize: number;
  setFontSize: (size: number) => Promise<void>;
  hydrateFontSize: (size: number) => void;
}

export const defaultDiffSettingsState = {
  fontSize: DEFAULT_DIFF_FONT_SIZE,
};

function assertDiffFontSize(size: number) {
  if (
    size < MIN_DIFF_FONT_SIZE ||
    size > MAX_DIFF_FONT_SIZE ||
    Number.isNaN(size)
  ) {
    throw new Error("Font size must be between 8 and 16");
  }
}

export const useDiffSettingsStore = create<DiffSettingsState>((set) => ({
  ...defaultDiffSettingsState,
  hydrateFontSize: (fontSize) => set({ fontSize }),
  setFontSize: async (size) => {
    assertDiffFontSize(size);
    set({ fontSize: size });
    await setSetting("diff_font_size", size.toString());
  },
}));
