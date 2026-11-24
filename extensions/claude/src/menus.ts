import { sessionId } from "./session";

/** A GitHub pull request: `repo` is `owner/name`. */
export type Pr = { repo: string; number: number };

/** The session and pull request of the row whose menu is open. */
export type Owner = { session: string | null; pr: Pr | null };

const MARK = "data-treq-item";
const enc = encodeURIComponent;

/** Opens Treq with the session as a terminal agent, offering workspaces for its pull requests. */
export const treqSessionUrl = (session: string, pr?: Pr | null): string =>
  `treq://claude/session?id=${enc(session)}${pr ? `&repo=${enc(pr.repo)}&pr=${pr.number}` : ""}`;

function parsePr(href: string | null | undefined): Pr | null {
  const m =
    href &&
    /^https:\/\/github\.com\/([\w.-]+\/[\w.-]+)\/pull\/(\d+)/.exec(href);
  return m ? { repo: m[1], number: Number(m[2]) } : null;
}

/** The first link matching `selector` in the nearest ancestor of `el` holding exactly one. */
function rowLink(el: Element, selector: string): string | null {
  for (
    let node: Element | null = el;
    node && node !== node.ownerDocument.body;
    node = node.parentElement
  ) {
    const links = node.matches(selector)
      ? [node]
      : [...node.querySelectorAll(selector)];
    if (links.length === 1) return links[0].getAttribute("href");
    if (links.length > 1) return null;
  }
  return null;
}

/**
 * The session and pull request of the row holding `target`, the menu
 * button just pressed: menus open in a portal, away from their row.
 */
export function menuOwner(target: Element): Owner {
  const session = rowLink(target, 'a[href*="/code/session_"]');
  return {
    session: session
      ? sessionId(new URL(session, "https://claude.ai").pathname)
      : null,
    pr: parsePr(
      rowLink(target, 'a[href^="https://github.com/"][href*="/pull/"]'),
    ),
  };
}

const text = (el: Element) => el.textContent?.replace(/\s+/g, " ").trim() ?? "";

/** A menu item styled like `like`, after it, that opens `url`. */
function addItem(
  like: Element,
  label: string,
  url: string,
  open: (url: string) => void,
): void {
  const item = like.cloneNode(false) as HTMLElement;
  for (const attr of [
    "id",
    "href",
    "aria-controls",
    "aria-expanded",
    "aria-haspopup",
    "data-radix-collection-item",
  ]) {
    item.removeAttribute(attr);
  }
  item.setAttribute(MARK, "");
  item.textContent = label;
  const choose = () => {
    open(url);
    like
      .closest('[role="menu"]')
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
  };
  item.addEventListener("click", choose);
  item.addEventListener("keydown", (e) => e.key === "Enter" && choose());
  like.after(item);
}

/**
 * Adds "Treq" to a session's "Open in" menu (beside "Terminal") and
 * "Open in Treq" to a pull request's menu (after "Open on GitHub").
 * Idempotent: re-run on every DOM change.
 */
export function decorateMenus(
  doc: Document,
  pathname: string,
  owner: Owner,
  open: (url: string) => void,
): void {
  for (const menu of doc.querySelectorAll('[role="menu"]')) {
    if (menu.querySelector(`[${MARK}]`)) continue;
    const menuItems = [...menu.querySelectorAll('[role="menuitem"]')];
    const terminal = menuItems.find((i) => /^Terminal\s*\d*$/.test(text(i)));
    const github = menuItems.find((i) => text(i) === "Open on GitHub");
    const session = owner.session ?? sessionId(pathname);
    if (!session) return;
    if (terminal) addItem(terminal, "Treq", treqSessionUrl(session), open);
    if (github) {
      const pr = parsePr(
        github.closest("a")?.getAttribute("href") ??
          github.querySelector("a")?.getAttribute("href"),
      );
      addItem(
        github,
        "Open in Treq",
        treqSessionUrl(sessionId(pathname) ?? session, pr ?? owner.pr),
        open,
      );
    }
  }
}
