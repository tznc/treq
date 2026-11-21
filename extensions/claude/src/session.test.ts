import { beforeEach, describe, expect, it } from "vitest";
import { applySessionTitle, sessionId, sessionTitle } from "./session";

beforeEach(() => {
  document.body.innerHTML = "";
  document.title = "Claude Code";
});

describe("sessionId", () => {
  it("reads the session a Claude Code page shows", () => {
    expect(sessionId("/code/session_012MZ5acdy")).toBe("session_012MZ5acdy");
    expect(sessionId("/code/session_012MZ5acdy/")).toBe("session_012MZ5acdy");
    expect(sessionId("/code")).toBeNull();
    expect(sessionId("/chat/session_1")).toBeNull();
  });
});

describe("sessionTitle", () => {
  it("reads the title from the session's link in the sidebar", () => {
    document.body.innerHTML = `<nav>
      <a href="/code/session_other">Other</a>
      <a href="/code/session_1"><span>  OIDC and JWT
        authentication </span><svg></svg></a></nav>`;
    expect(sessionTitle(document, "session_1")).toBe("OIDC and JWT authentication");
  });

  it("has none while the sidebar does not list the session", () => {
    expect(sessionTitle(document, "session_1")).toBeNull();
  });
});

describe("applySessionTitle", () => {
  it("names the tab after the session", () => {
    document.body.innerHTML = `<a href="/code/session_1">Fix login</a>`;
    applySessionTitle(document, "/code/session_1");
    expect(document.title).toBe("Fix login · Claude Code");
  });

  it("leaves the title alone off a session or without its title", () => {
    applySessionTitle(document, "/code");
    applySessionTitle(document, "/code/session_1");
    expect(document.title).toBe("Claude Code");
  });

  it("writes the title only when it changes, so observing it does not loop", () => {
    document.body.innerHTML = `<a href="/code/session_1">Fix login</a>`;
    applySessionTitle(document, "/code/session_1");
    let writes = 0;
    new MutationObserver(() => writes++).observe(document.head, { subtree: true, childList: true, characterData: true });
    applySessionTitle(document, "/code/session_1");
    return Promise.resolve().then(() => expect(writes).toBe(0));
  });
});
