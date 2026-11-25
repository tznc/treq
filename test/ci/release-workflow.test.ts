import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = readFileSync(
  join(import.meta.dirname, "../../.github/workflows/release.yml"),
  "utf8",
);

// The `prepare` job's steps, each as its own block of YAML text.
const prepareSteps = () => {
  const job = SRC.split(/^ {2}prepare:\n/m)[1]?.split(/^ {2}\S/m)[0] ?? "";
  return job.split(/^ {6}- /m).slice(1);
};

// Drafts own no tag, so GitHub accepts any number of them for one version.
// A step that runs when the gate skips adds another draft on every run.
describe("release.yml prepare job", () => {
  it("gates every step after the version check on should_run", () => {
    const steps = prepareSteps();
    const gate = steps.findIndex((s) => /^ {8}id: gate$/m.test(s));
    expect(gate).toBeGreaterThanOrEqual(0);

    const after = steps.slice(gate + 1);
    expect(after.length).toBeGreaterThan(0);
    for (const step of after) {
      expect(step).toMatch(
        /^ {8}if: steps\.gate\.outputs\.should_run == 'true'$/m,
      );
    }
  });
});
