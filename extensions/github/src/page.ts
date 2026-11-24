/** The GitHub page the extension decorates. `repo` is `owner/name`. */
export type Page =
  | { kind: "repo"; repo: string }
  | { kind: "pr"; repo: string; number: number }
  | { kind: "other"; repo: string };

// GitHub owner and repository names: letters, digits, `-`, `_` and `.`,
// never `.` or `..` alone.
const NAME = /^(?!\.{1,2}$)[A-Za-z0-9._-]+$/;

/** The repository page `pathname` shows, or null for any other page. */
export function parsePage(pathname: string): Page | null {
  const [owner, name, section, id] = pathname.split("/").filter(Boolean);
  if (!owner || !name || !NAME.test(owner) || !NAME.test(name)) return null;
  const repo = `${owner}/${name}`;
  if (!section) return { kind: "repo", repo };
  if (section !== "pull") return { kind: "other", repo };
  if (!id || !/^[1-9]\d*$/.test(id)) return null;
  return { kind: "pr", repo, number: Number(id) };
}

/**
 * The repository page `pathname` shows, when GitHub's own page metadata
 * names that repository: account pages such as `/settings/profile` share
 * the `/owner/name` shape.
 */
export function currentPage(doc: Document, pathname: string): Page | null {
  const page = parsePage(pathname);
  const nwo = doc.querySelector<HTMLMetaElement>(
    'meta[name="octolytics-dimension-repository_nwo"]',
  )?.content;
  return page && nwo?.toLowerCase() === page.repo.toLowerCase() ? page : null;
}
