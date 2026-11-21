/** The file and line of the diff row holding `target`: the new side, else the old. */
export function diffLine(target: Element): { path: string; line: number } | null {
  const numbers = target.closest("tr")?.querySelectorAll("td[data-line-number]");
  const path = target.closest("[data-tagsearch-path]")?.getAttribute("data-tagsearch-path");
  const last = numbers?.[numbers.length - 1]?.getAttribute("data-line-number");
  return path && last ? { path, line: Number(last) } : null;
}

/** The blob view's line number drawn at height `y`; code there sits under a textarea. */
export function blobLine(doc: Document, y: number): HTMLElement | null {
  for (const el of doc.querySelectorAll<HTMLElement>(".react-code-file-contents .react-line-number[data-line-number]")) {
    const { top, bottom } = el.getBoundingClientRect();
    if (y >= top && y < bottom) return el;
  }
  return null;
}

/**
 * The ref and path a `/owner/repo/blob/{ref}/{path}` URL names. Branch names
 * may hold `/`, so GitHub's page data splits them when it is for this file.
 */
export function blobFile(doc: Document, pathname: string): { ref: string; path: string } | null {
  const rest = pathname.split("/").slice(3);
  if (rest[0] !== "blob" || rest.length < 3) return null;
  const tail = rest.slice(1).map(decodeURIComponent).join("/");
  try {
    const data = doc.querySelector('script[data-target="react-app.embeddedData"]')?.textContent;
    const route = data && JSON.parse(data).payload?.codeViewLayoutRoute;
    const { path, refInfo } = route ?? {};
    if (path && refInfo?.name && tail === `${refInfo.name}/${path}`) return { ref: refInfo.name, path };
  } catch {
    // Unreadable page data: fall back to the URL.
  }
  return { ref: rest[1], path: rest.slice(2).map(decodeURIComponent).join("/") };
}

/** A pull request's head branch: the second ref in its header (base, then head). */
export function prHeadRef(doc: Document): string | null {
  const refs = doc.querySelectorAll('a[class*="BranchName"], .commit-ref.css-truncate');
  return refs[1]?.textContent?.trim() || null;
}
