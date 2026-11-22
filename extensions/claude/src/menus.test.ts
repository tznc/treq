import { beforeEach, describe, expect, it, vi } from "vitest";
import { decorateMenus, menuOwner, treqSessionUrl } from "./menus";

let open: ReturnType<typeof vi.fn>;
beforeEach(() => {
  document.body.innerHTML = "";
  open = vi.fn();
});

const items = () =>
  [...document.querySelectorAll('[role="menuitem"]')].map((i) =>
    i.textContent?.trim(),
  );

describe("treqSessionUrl", () => {
  it("names the session, and a pull request when there is one", () => {
    expect(treqSessionUrl("session_1")).toBe(
      "treq://claude/session?id=session_1",
    );
    expect(treqSessionUrl("session_1", { repo: "tznc/treq", number: 7 })).toBe(
      "treq://claude/session?id=session_1&repo=tznc%2Ftreq&pr=7",
    );
  });
});

describe("menuOwner", () => {
  it("finds the session and pull request of the row whose menu button was pressed", () => {
    document.body.innerHTML = `<div><div id="row"><a href="/code/session_A">Fix</a>
      <a href="https://github.com/tznc/treq/pull/9">treq #9</a><button id="more">…</button></div>
      <a href="/code/session_B">Other</a></div>`;
    expect(menuOwner(document.querySelector("#more")!)).toEqual({
      session: "session_A",
      pr: { repo: "tznc/treq", number: 9 },
    });
  });

  it("has nothing for a button outside any row", () => {
    document.body.innerHTML = `<button id="b"></button>`;
    expect(menuOwner(document.querySelector("#b")!)).toEqual({
      session: null,
      pr: null,
    });
  });
});

describe("decorateMenus", () => {
  const openIn = `<div role="menu"><div role="menuitem" class="item">Terminal<kbd>1</kbd></div></div>`;
  const prMenu = (openOnGitHub: string) =>
    `<div role="menu">${openOnGitHub}<div role="menuitem">Copy link</div></div>`;

  it("adds Treq to a session row's Open in menu, for that row's session", () => {
    document.body.innerHTML = openIn;
    decorateMenus(
      document,
      "/code/session_X",
      { session: "session_A", pr: null },
      open,
    );
    expect(items()).toEqual(["Terminal1", "Treq"]);
    expect(document.querySelectorAll(".item")).toHaveLength(2);
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[1].click();
    expect(open).toHaveBeenCalledWith("treq://claude/session?id=session_A");
  });

  it("falls back to the open session", () => {
    document.body.innerHTML = openIn;
    decorateMenus(
      document,
      "/code/session_X",
      { session: null, pr: null },
      open,
    );
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[1].click();
    expect(open).toHaveBeenCalledWith("treq://claude/session?id=session_X");
  });

  it("adds Open in Treq to a pull request's menu, after Open on GitHub", () => {
    document.body.innerHTML = prMenu(
      `<a role="menuitem" href="https://github.com/tznc/treq/pull/1697">Open on GitHub</a>`,
    );
    decorateMenus(
      document,
      "/code/session_X",
      { session: null, pr: null },
      open,
    );
    expect(items()).toEqual(["Open on GitHub", "Open in Treq", "Copy link"]);
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[1].click();
    expect(open).toHaveBeenCalledWith(
      "treq://claude/session?id=session_X&repo=tznc%2Ftreq&pr=1697",
    );
  });

  it("takes the pull request from its row when the menu has no link", () => {
    document.body.innerHTML = prMenu(
      `<div role="menuitem">Open on GitHub</div>`,
    );
    decorateMenus(
      document,
      "/code/session_X",
      { session: null, pr: { repo: "tznc/treq", number: 3 } },
      open,
    );
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[1].click();
    expect(open).toHaveBeenCalledWith(
      "treq://claude/session?id=session_X&repo=tznc%2Ftreq&pr=3",
    );
  });

  it("adds each item once and leaves other menus alone", () => {
    document.body.innerHTML = `${openIn}<div role="menu"><div role="menuitem">Pin</div></div>`;
    decorateMenus(
      document,
      "/code/session_X",
      { session: null, pr: null },
      open,
    );
    decorateMenus(
      document,
      "/code/session_X",
      { session: null, pr: null },
      open,
    );
    expect(items()).toEqual(["Terminal1", "Treq", "Pin"]);
  });

  it("adds nothing without a session", () => {
    document.body.innerHTML = openIn;
    decorateMenus(document, "/code", { session: null, pr: null }, open);
    expect(items()).toEqual(["Terminal1"]);
  });
});
