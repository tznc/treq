import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const ROOT = join(import.meta.dirname, "../..");
const AST_GREP = join(ROOT, "node_modules/.bin/ast-grep");

const pkg: { scripts: Record<string, string> } = JSON.parse(
  readFileSync(join(ROOT, "package.json"), "utf8"),
);
const deployWeb = readFileSync(
  join(ROOT, ".github/workflows/deploy-web.yml"),
  "utf8",
);

// The full scans CI runs; `--filter` scans re-run a subset of the same rules.
const fullScans = [
  ...`${pkg.scripts.lint} && ${pkg.scripts["lint:e2e"]}`.split("&&"),
  ...deployWeb.split("\n"),
]
  .map((line) => line.trim())
  .filter(
    (line) => line.includes("ast-grep scan") && !line.includes("--filter"),
  );

function scanWarningRule(args: string[]) {
  const dir = mkdtempSync(join(tmpdir(), "sg-gate-"));
  mkdirSync(join(dir, "rules"));
  writeFileSync(join(dir, "sgconfig.yml"), "ruleDirs:\n  - rules\n");
  writeFileSync(
    join(dir, "rules/warn.yml"),
    "id: warn\nlanguage: js\nseverity: warning\nrule:\n  pattern: console.log($A)\n",
  );
  writeFileSync(join(dir, "a.js"), "console.log(1);\n");
  return spawnSync(AST_GREP, ["scan", ...args], { cwd: dir, encoding: "utf8" });
}

describe("ast-grep CI gate", () => {
  it("finds the full scans in package.json and deploy-web.yml", () => {
    expect(fullScans.length).toBeGreaterThanOrEqual(3);
  });

  it("runs every full scan with --error", () => {
    for (const scan of fullScans)
      expect(scan).toMatch(/ast-grep scan --error\b/);
  });

  it("fails a scan on a warning-severity hit only with --error", () => {
    expect(scanWarningRule([]).status).toBe(0);
    expect(scanWarningRule(["--error"]).status).toBe(1);
  });
});
