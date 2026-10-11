type KeyboardShortcutEntry = {
  keys: string[];
  action: string;
};

export type KeyboardShortcutSection = {
  title: string;
  shortcuts: KeyboardShortcutEntry[];
};

/** Canonical keyboard shortcut reference shown in the ? modal and docs. */
export const KEYBOARD_SHORTCUT_SECTIONS: KeyboardShortcutSection[] = [
  {
    title: "Global",
    shortcuts: [
      { keys: ["⌘", "N"], action: "New workspace" },
      { keys: ["⌘", "⇧", "N"], action: "Create stacked workspace" },
      { keys: ["⌘", "K"], action: "Command Palette" },
      { keys: ["⌘", "P"], action: "File Search" },
      { keys: ["?"], action: "Show keyboard shortcuts" },
      { keys: ["Esc"], action: "Close dialog" },
    ],
  },
  {
    title: "Terminal",
    shortcuts: [
      { keys: ["⌘", "J"], action: "Toggle terminal pane" },
      { keys: ["⌘", "Ctrl", "J"], action: "Maximize/restore terminal" },
      { keys: ["⌘", "]"], action: "New agent terminal" },
      { keys: ["⌘", "\\"], action: "New shell terminal" },
      { keys: ["⌘", "W"], action: "Close selected terminal" },
      { keys: ["⌘", "C"], action: "Copy selection (or interrupt)" },
      { keys: ["⌘", "V"], action: "Paste" },
    ],
  },
  {
    title: "Files & Diff",
    shortcuts: [
      { keys: ["⌘", "F"], action: "Search in file or diff" },
      { keys: ["⌘", "A"], action: "Select all changed files" },
    ],
  },
];
