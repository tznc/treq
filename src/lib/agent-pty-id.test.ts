import { describe, expect, it } from "vitest";
import { agentPtySessionId, paneSessionId } from "./agent-pty-id";

describe("agentPtySessionId", () => {
  it("differs for the same session id in different repositories", () => {
    expect(agentPtySessionId("/repos/app", 1)).not.toBe(
      agentPtySessionId("/repos/api", 1),
    );
  });

  it("is stable for the same repository and session", () => {
    expect(agentPtySessionId("/repos/app", 7)).toBe(
      agentPtySessionId("/repos/app", 7),
    );
  });

  it("uses only characters Tauri accepts in event names", () => {
    expect(agentPtySessionId("/repos/ümlaut app/x", 3)).toMatch(
      /^session-[a-z0-9]+-3$/,
    );
  });
});

describe("paneSessionId", () => {
  it("keeps main-repository ids unchanged", () => {
    expect(paneSessionId("/repos/app", "/repos/app", 4)).toBe(4);
  });

  it("gives equal ids from different repositories different pane ids", () => {
    const main = paneSessionId("/repos/app", "/repos/app", 1);
    const api = paneSessionId("/repos/api", "/repos/app", 1);
    const web = paneSessionId("/repos/web", "/repos/app", 1);
    expect(new Set([main, api, web]).size).toBe(3);
    expect(Number.isSafeInteger(api)).toBe(true);
  });
});
