import { describe, expect, it } from "vitest";
import type { RunSummary, WorkflowInfo } from "./api-types";
import { summarizeChecks } from "./checks-summary";

const wf = (filename: string): WorkflowInfo => ({
  filename,
  name: filename,
  jobs: [],
});
const run = (status: string): RunSummary => ({
  id: 1,
  filename: "x",
  status,
  started_at: "",
  completed_at: null,
  jobs: [],
});

describe("summarizeChecks", () => {
  it("is none without workflows", () => {
    expect(summarizeChecks([], {}, []).state).toBe("none");
  });

  it("is not-run until every workflow has a run", () => {
    const summary = summarizeChecks(
      [wf("a"), wf("b")],
      { a: run("passed") },
      [],
    );
    expect(summary).toMatchObject({ state: "not-run", passed: 1, total: 2 });
  });

  it("is passed when every latest run passed", () => {
    expect(summarizeChecks([wf("a")], { a: run("passed") }, []).state).toBe(
      "passed",
    );
  });

  it("prefers failed over passed and running over failed", () => {
    const workflows = [wf("a"), wf("b")];
    const runs = { a: run("passed"), b: run("failed") };
    expect(summarizeChecks(workflows, runs, []).state).toBe("failed");
    expect(summarizeChecks(workflows, runs, ["a"])).toMatchObject({
      state: "running",
      running: 1,
      failed: 1,
      passed: 0,
    });
  });
});
