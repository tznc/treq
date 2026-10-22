import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const REPO_ROOT = join(import.meta.dirname, "..");
const PRD_PATH = join(REPO_ROOT, "prds/remote-development.md");
const MATRIX_PATH = join(REPO_ROOT, "src-tauri/tests/remote_e2e_README.md");

function acceptanceCriteriaSection(markdown: string): string {
  const heading = "## Ship-blocking acceptance criteria";
  const start = markdown.indexOf(heading);
  if (start === -1) {
    throw new Error(`Missing "${heading}" section in ${PRD_PATH}`);
  }
  const afterHeading = markdown.slice(start + heading.length);
  const nextSection = afterHeading.search(/\n## /);
  return nextSection === -1 ? afterHeading : afterHeading.slice(0, nextSection);
}

function countPrdAcceptanceCriteria(markdown: string): number {
  const section = acceptanceCriteriaSection(markdown);
  return section.match(/^\d+\.\s+/gm)?.length ?? 0;
}

function matrixSection(markdown: string): string {
  const heading = "## Acceptance criteria mapping";
  const start = markdown.indexOf(heading);
  if (start === -1) {
    throw new Error(`Missing "${heading}" section in ${MATRIX_PATH}`);
  }
  const afterHeading = markdown.slice(start + heading.length);
  const nextSection = afterHeading.search(/\n## /);
  return nextSection === -1 ? afterHeading : afterHeading.slice(0, nextSection);
}

function countReadmeMatrixRows(markdown: string): number {
  const section = matrixSection(markdown);
  return section.match(/^\| \d+ \|/gm)?.length ?? 0;
}

describe("remote development acceptance traceability", () => {
  it("documents the same number of criteria as prds/remote-development.md", () => {
    const prd = readFileSync(PRD_PATH, "utf8");
    const matrix = readFileSync(MATRIX_PATH, "utf8");
    const prdCount = countPrdAcceptanceCriteria(prd);
    const matrixCount = countReadmeMatrixRows(matrix);
    expect(prdCount).toBeGreaterThan(0);
    expect(matrixCount).toBe(prdCount);
  });
});
