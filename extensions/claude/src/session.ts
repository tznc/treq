const SUFFIX = " · Claude Code";

/** The session a `/code/{session id}` page shows, or null. */
export function sessionId(pathname: string): string | null {
  return /^\/code\/(session_[A-Za-z0-9]+)\/?$/.exec(pathname)?.[1] ?? null;
}

/** The session's title, from its link in the sidebar; null while it is not listed. */
export function sessionTitle(doc: Document, id: string): string | null {
  for (const link of doc.querySelectorAll<HTMLAnchorElement>(
    `a[href$="/code/${id}"]`,
  )) {
    const title = link.textContent?.replace(/\s+/g, " ").trim();
    if (title) return title;
  }
  return null;
}

/** Names the tab after the session `pathname` shows. Writes only on change. */
export function applySessionTitle(doc: Document, pathname: string): void {
  const id = sessionId(pathname);
  const title = id && sessionTitle(doc, id);
  if (title && doc.title !== title + SUFFIX) doc.title = title + SUFFIX;
}
