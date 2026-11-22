import type { CodeLine } from "./prompt";

const rowLine = (row: Element): CodeLine | null => {
  const numbers = row.querySelectorAll("td[data-line-number]");
  const line = numbers[numbers.length - 1]?.getAttribute("data-line-number");
  const code = row.querySelector(".blob-code-inner");
  const text = `${code?.getAttribute("data-code-marker") ?? ""}${code?.textContent ?? ""}`;
  return line ? { line: Number(line), text: text.replace(/\n$/, "") } : null;
};

/**
 * The file and lines of the diff row holding `target` (each row's new side,
 * else its old, its code after its ` `, `+` or `-` marker): every selected
 * row when it is one of them, else its own.
 */
export function diffContext(
  target: Element,
): { path: string; lines: CodeLine[] } | null {
  const file = target.closest("[data-tagsearch-path]");
  const row = target.closest("tr");
  const path = file?.getAttribute("data-tagsearch-path");
  if (!file || !row || !path || !rowLine(row)) return null;
  const selected = [...file.querySelectorAll("tr")].filter((r) =>
    r.querySelector("td.selected-line"),
  );
  const rows = selected.includes(row) ? selected : [row];
  return {
    path,
    lines: rows.map(rowLine).filter((l): l is CodeLine => l !== null),
  };
}

/**
 * Line `line` of the blob view, or the selected lines (`#L2-L4`) when it is
 * one of them. The view draws only lines on screen; its hidden textarea
 * holds the whole file.
 */
export function blobContext(
  doc: Document,
  line: number,
  hash: string,
): CodeLine[] {
  const m = /^#L(\d+)(?:-L(\d+))?$/.exec(hash);
  const [start, end] = m ? [Number(m[1]), Number(m[2] ?? m[1])] : [line, line];
  const [from, to] = line >= start && line <= end ? [start, end] : [line, line];
  const file = doc
    .querySelector<HTMLTextAreaElement>("#read-only-cursor-text-area")
    ?.value.split("\n");
  const lines: CodeLine[] = [];
  for (let n = from; n <= to; n++) {
    lines.push({
      line: n,
      text: file?.[n - 1] ?? doc.getElementById(`LC${n}`)?.textContent ?? "",
    });
  }
  return lines;
}

/** The blob view's line number drawn at height `y`; code there sits under a textarea. */
export function blobLine(doc: Document, y: number): HTMLElement | null {
  for (const el of doc.querySelectorAll<HTMLElement>(
    ".react-code-file-contents .react-line-number[data-line-number]",
  )) {
    const { top, bottom } = el.getBoundingClientRect();
    if (y >= top && y < bottom) return el;
  }
  return null;
}

/**
 * The ref and path a `/owner/repo/blob/{ref}/{path}` URL names. Branch names
 * may hold `/`, so GitHub's page data splits them when it is for this file.
 */
export function blobFile(
  doc: Document,
  pathname: string,
): { ref: string; path: string } | null {
  const rest = pathname.split("/").slice(3);
  if (rest[0] !== "blob" || rest.length < 3) return null;
  const tail = rest.slice(1).map(decodeURIComponent).join("/");
  try {
    const data = doc.querySelector(
      'script[data-target="react-app.embeddedData"]',
    )?.textContent;
    const route = data && JSON.parse(data).payload?.codeViewLayoutRoute;
    const { path, refInfo } = route ?? {};
    if (path && refInfo?.name && tail === `${refInfo.name}/${path}`)
      return { ref: refInfo.name, path };
  } catch {
    // Unreadable page data: fall back to the URL.
  }
  return {
    ref: rest[1],
    path: rest.slice(2).map(decodeURIComponent).join("/"),
  };
}

/** A pull request's head branch: the second ref in its header (base, then head). */
export function prHeadRef(doc: Document): string | null {
  const refs = doc.querySelectorAll(
    'a[class*="BranchName"], .commit-ref.css-truncate',
  );
  return refs[1]?.textContent?.trim() || null;
}
