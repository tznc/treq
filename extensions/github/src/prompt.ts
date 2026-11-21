/** A numbered line of code. */
export type CodeLine = { line: number; text: string };

/** Lines of a file on a ref of a GitHub repository (`owner/name`). */
export type CodeAt = { repo: string; ref: string; path: string; lines: CodeLine[] };

/** At most this many lines are embedded; longer ranges keep the ends. */
const MAX_LINES = 10;

/** `text` after the file, line numbers and code it is about. */
export function codePrompt(at: CodeAt, text: string): string {
  const first = at.lines[0].line;
  const last = at.lines[at.lines.length - 1].line;
  const range = first === last ? `${first}` : `${first}-${last}`;
  const width = String(Math.max(first, last)).length;
  const numbered = at.lines.map((l) => `${String(l.line).padStart(width)}  ${l.text}`);
  const half = MAX_LINES / 2;
  const code = numbered.length > MAX_LINES ? [...numbered.slice(0, half), "…", ...numbered.slice(-half)] : numbered;
  return `${at.path}:${range} (${at.repo}@${at.ref})\n\`\`\`\n${code.join("\n")}\n\`\`\`\n\n${text}`;
}
