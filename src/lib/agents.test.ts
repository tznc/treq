import { describe, expect, it } from "vitest";
import { AGENTS, agentInfo, isAgentKind, toAgentKind } from "./agents";

describe("agents", () => {
  it("lists each supported agent once", () => {
    const ids = AGENTS.map((agent) => agent.id);
    expect(ids).toEqual(["claude", "codex", "cursor", "copilot"]);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("recognizes supported agent ids only", () => {
    expect(isAgentKind("copilot")).toBe(true);
    expect(isAgentKind("cursor-agent")).toBe(false);
    expect(isAgentKind("")).toBe(false);
    expect(isAgentKind(undefined)).toBe(false);
  });

  it("narrows a stored setting, dropping unknown values", () => {
    expect(toAgentKind("codex")).toBe("codex");
    expect(toAgentKind("gpt")).toBeUndefined();
    expect(toAgentKind(null)).toBeUndefined();
  });

  it("describes each agent's label and capabilities", () => {
    expect(agentInfo("copilot")).toMatchObject({
      label: "Copilot",
      hasPlanMode: false,
    });
    expect(agentInfo("claude")).toMatchObject({
      label: "Claude",
      hasPlanMode: true,
    });
  });
});
