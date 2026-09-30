import { describe, expect, it } from "vitest";
import { agentPtySessionId } from "./agent-pty-id";

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
