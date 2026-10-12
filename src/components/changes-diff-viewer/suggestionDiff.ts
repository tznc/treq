import { diffLines, diffWordsWithSpace } from "diff";

/** One inline run within a suggestion diff row. */
interface SuggestionDiffSegment {
  text: string;
  /** True when this run is the part that actually changed on the row. */
  changed: boolean;
}

type SuggestionDiffRowType = "unchanged" | "removed" | "added";

/** One rendered row of a suggestion diff, in GitHub suggestion style. */
export interface SuggestionDiffRow {
  type: SuggestionDiffRowType;
  segments: SuggestionDiffSegment[];
}

/**
 * A removed/added pair only gets word-level highlighting when the rewrite is
 * small relative to the line; past this share of changed characters the whole
 * line reads better as a plain red/green swap.
 */
const WORD_LEVEL_CHANGE_LIMIT = 0.5;

const splitLines = (text: string): string[] => {
  const lines = text.split("\n");
  // A trailing newline is a line terminator, not an extra empty line.
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
};

const plainRow = (
  type: SuggestionDiffRowType,
  text: string,
): SuggestionDiffRow => ({
  type,
  segments: [{ text, changed: type !== "unchanged" }],
});

/**
 * Word-level diff of one replaced line, or null when too much of the line
 * changed for the highlighting to help.
 */
function wordRows(
  removedLine: string,
  addedLine: string,
): [SuggestionDiffRow, SuggestionDiffRow] | null {
  const parts = diffWordsWithSpace(removedLine, addedLine);
  const changedChars = parts
    .filter((part) => part.added || part.removed)
    .reduce((total, part) => total + part.value.length, 0);
  const longest = Math.max(removedLine.length, addedLine.length, 1);
  if (changedChars / longest > WORD_LEVEL_CHANGE_LIMIT) return null;

  const removed: SuggestionDiffSegment[] = [];
  const added: SuggestionDiffSegment[] = [];
  for (const part of parts) {
    if (part.added) added.push({ text: part.value, changed: true });
    else if (part.removed) removed.push({ text: part.value, changed: true });
    else {
      removed.push({ text: part.value, changed: false });
      added.push({ text: part.value, changed: false });
    }
  }
  return [
    { type: "removed", segments: removed },
    { type: "added", segments: added },
  ];
}

/**
 * Render `suggested` as a +/- diff against `original`, the way GitHub shows an
 * inline suggestion: unchanged lines plain, replaced lines as a red/green pair
 * with the changed words highlighted inside them when the edit is small.
 *
 * Returns null when there is nothing useful to show (no original text, or the
 * suggestion is identical to it), so the caller can fall back to plain text.
 */
export function buildSuggestionDiff(
  original: string | null | undefined,
  suggested: string,
): SuggestionDiffRow[] | null {
  if (original == null || original === "") return null;
  if (original === suggested) return null;

  const rows: SuggestionDiffRow[] = [];
  const parts = diffLines(original, suggested);

  for (let index = 0; index < parts.length; index++) {
    const part = parts[index];
    const lines = splitLines(part.value);

    if (!part.added && !part.removed) {
      for (const line of lines) rows.push(plainRow("unchanged", line));
      continue;
    }

    if (part.removed) {
      const next = parts[index + 1];
      const addedLines = next?.added ? splitLines(next.value) : null;
      // Only pair up blocks that line up one-for-one; anything else is a
      // structural rewrite and reads better as separate red and green blocks.
      if (addedLines && addedLines.length === lines.length) {
        lines.forEach((line, offset) => {
          rows.push(
            ...(wordRows(line, addedLines[offset]) ?? [
              plainRow("removed", line),
              plainRow("added", addedLines[offset]),
            ]),
          );
        });
        index++;
        continue;
      }
      for (const line of lines) rows.push(plainRow("removed", line));
      continue;
    }

    for (const line of lines) rows.push(plainRow("added", line));
  }

  return rows.length > 0 ? rows : null;
}
