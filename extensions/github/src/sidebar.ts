import { CODEX_WEB_URL, claudeUrl, treqPrUrl, treqRepoUrl } from "./links";
import type { Page } from "./page";

const ID = "treq-open-in";

function linksFor(page: Page): [string, string][] | null {
  if (page.kind === "other") return null;
  const treq = page.kind === "pr" ? treqPrUrl(page.repo, page.number) : treqRepoUrl(page.repo);
  return [
    ["Open in Claude", claudeUrl(page.repo)],
    ["Open in Codex", CODEX_WEB_URL],
    ["Open in Treq", treq],
  ];
}

/** Where the links go: the end of the page's sidebar, if it has one. */
function sidebar(doc: Document, page: Page): { parent: Element; className: string } | null {
  if (page.kind === "pr") {
    const parent = doc.querySelector("#partial-discussion-sidebar");
    return parent && { parent, className: "discussion-sidebar-item" };
  }
  const grid = doc.querySelector('[data-component="SplitPageLayout.Pane"] h2')?.parentElement?.parentElement;
  return grid ? { parent: grid, className: "" } : null;
}

/**
 * Shows the "Open in" links for `page`: at the end of its sidebar, or
 * floating bottom right without one. Idempotent; re-run on every DOM change.
 */
export function renderOpenIn(doc: Document, page: Page): void {
  const links = linksFor(page);
  const existing = doc.getElementById(ID);
  const key = links ? JSON.stringify(links) : "";
  if (existing?.dataset.key === key && existing.isConnected) return;
  existing?.remove();
  if (!links) return;

  const section = doc.createElement("div");
  section.id = ID;
  section.dataset.key = key;
  const target = sidebar(doc, page);
  section.className = target ? `treq-open-in ${target.className}`.trim() : "treq-open-in treq-open-in--floating";
  for (const [label, href] of links) {
    const a = doc.createElement("a");
    a.className = "treq-open-in__link";
    a.href = href;
    a.textContent = label;
    if (href.startsWith("https:")) {
      a.target = "_blank";
      a.rel = "noopener";
    }
    section.append(a);
  }
  (target?.parent ?? doc.body).append(section);
}
