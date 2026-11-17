import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const DIR = join(import.meta.dirname, "../../.github/workflows");
const workflows = readdirSync(DIR)
  .filter((f) => f.endsWith(".yml"))
  .map((f) => [f, readFileSync(join(DIR, f), "utf8")] as const);

// Every check runs on drafts, so marking a PR ready for review must not
// start CI again.
describe("PR workflows", () => {
  it.each(workflows)("%s does not run on ready_for_review", (_, src) => {
    expect(src).not.toMatch(/ready_for_review/);
  });

  it.each(workflows)("%s does not skip draft PRs", (_, src) => {
    expect(src).not.toMatch(/pull_request\.draft/);
  });
});
