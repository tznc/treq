/**
 * Writes a plain-English outcome checklist next to each service-qa run:
 * Vitest asserts prove the contract; this file is the agent-facing checklist
 * for step 5 of the skill.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATED_DIR = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  ".generated",
);

export type OutcomeOptions = {
  /** Non-empty checklist of claims a reviewer should confirm from this run. */
  expectations: string[];
  /** Optional structured payload (RPC result, HTTP status, ids) for the report. */
  details?: Record<string, unknown>;
};

export async function recordOutcome(
  name: string,
  options: OutcomeOptions,
): Promise<string> {
  const { expectations, details } = options;

  if (!expectations || expectations.length === 0) {
    throw new Error(
      `recordOutcome("${name}") requires a non-empty "expectations" list — ` +
        "plain-English claims about what this service check proved. See the service-qa skill.",
    );
  }

  if (expectations.length > 3) {
    throw new Error(
      `recordOutcome("${name}") allows at most 3 expectations (got ${expectations.length}). ` +
        "Split into a second outcome with its own name.",
    );
  }

  fs.mkdirSync(GENERATED_DIR, { recursive: true });
  const outPath = path.join(GENERATED_DIR, `${name}.json`);
  const payload = {
    name,
    recordedAt: new Date().toISOString(),
    expectations,
    details: details ?? {},
  };
  fs.writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
  return outPath;
}
