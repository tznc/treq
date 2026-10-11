// Line-level primitives shared by every diff viewer (Changes, Commits,
// stash, mobile): hunk header parsing, line numbering and +/- styling.
import type { JjDiffHunk } from "../../lib/api";

export const getLineTypeClass = (line: string): string => {
  if (line.startsWith("+")) return "bg-emerald-500/20";
  if (line.startsWith("-")) return "bg-red-500/20";
  return "";
};

export const getLinePrefix = (line: string): string => {
  if (line.startsWith("+")) return "+";
  if (line.startsWith("-")) return "-";
  return " ";
};

export const parseHunkHeader = (
  header: string,
): {
  newCount: number;
  newStart: number;
  oldCount: number;
  oldStart: number;
} => {
  const match = header.match(/@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
  if (!match) {
    return { newCount: 1, newStart: 1, oldCount: 1, oldStart: 1 };
  }
  return {
    newCount: match[4] ? parseInt(match[4], 10) : 1,
    newStart: parseInt(match[3], 10),
    oldCount: match[2] ? parseInt(match[2], 10) : 1,
    oldStart: parseInt(match[1], 10),
  };
};

export const computeHunkLineNumbers = (
  hunk: JjDiffHunk,
): Array<{ new?: number; old?: number }> => {
  const { oldStart, newStart } = parseHunkHeader(hunk.header);
  let oldLine = oldStart;
  let newLine = newStart;

  return hunk.lines.map((line) => {
    if (line.startsWith("+")) {
      return { new: newLine++ };
    } else if (line.startsWith("-")) {
      return { old: oldLine++ };
    } else {
      return { new: newLine++, old: oldLine++ };
    }
  });
};
