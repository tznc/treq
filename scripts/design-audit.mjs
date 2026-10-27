#!/usr/bin/env node
// Code half of the /design-audit skill: how far the GPUI screens have
// drifted from the design system (crates/treq-ui).
//
// For each design lint it reports:
// - new drift: violations in files the rule is not ignoring (CI fails on these);
// - debt: violations left in files on the rule's "Not migrated yet" list;
// - stale ignores: listed files that already pass and can come off the list.
// Then how often screens use the `ui` components.
//
//   node scripts/design-audit.mjs          # Markdown
//   node scripts/design-audit.mjs --json   # for comparing runs

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { parse, stringify } from "yaml";

const root = new URL("..", import.meta.url).pathname;
const RULES = [
  "gpui-hover-token",
  "gpui-no-raw-color",
  "gpui-no-raw-text-size",
  "gpui-no-zpx-spacing",
  "gpui-escape-via-layers",
  "gpui-no-adhoc-controls",
  "gpui-button-no-restyle",
  "ui-copy-no-yes-no",
];
const NOT_MIGRATED = /^\s*#\s*Not migrated yet/;
const SCREENS = "src-tauri/src/gpui_shell";
// The `ui` components a screen should build from, by constructor.
const COMPONENTS = [
  "Button",
  "IconButton",
  "SplitButton",
  "Picker",
  "Badge",
  "Count",
  "Dot",
  "Kbd",
  "Alert",
  "Tabs",
  "Row",
  "MenuItem",
  "Popover",
  "Dialog",
  "ConfirmDialog",
  "EmptyState",
  "Loading",
  "Skeleton",
];

/** The rule's ignore entries under its "Not migrated yet" comment. */
function notMigrated(text) {
  const files = [];
  let inList = false;
  for (const line of text.split("\n")) {
    if (NOT_MIGRATED.test(line)) inList = true;
    else if (/^\s*#/.test(line) || !/^\s*-\s/.test(line)) inList = false;
    else if (inList && !line.includes("*")) {
      // Globs (tests) are permanent exemptions, not migration debt.
      files.push(line.replace(/^\s*-\s*["']?|["']?\s*$/g, ""));
    }
  }
  return files;
}

/** Matches of `rule` with its "Not migrated yet" ignores removed. */
function scan(rule, debtFiles, dir) {
  const path = join(dir, `${rule.id}.yml`);
  const ignores = (rule.ignores ?? []).filter((f) => !debtFiles.includes(f));
  writeFileSync(path, stringify({ ...rule, ignores }));
  let out;
  try {
    out = execFileSync("npx", ["ast-grep", "scan", "--rule", path, "--json=compact"], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      maxBuffer: 64 * 1024 * 1024,
    });
  } catch (err) {
    // ast-grep exits 1 when an error-severity rule matches.
    out = err.stdout;
  }
  return JSON.parse(out || "[]").map((m) => m.file);
}

function rustFiles(dir) {
  return readdirSync(join(root, dir), { recursive: true })
    .filter((f) => f.endsWith(".rs"))
    .map((f) => join(dir, f));
}

function adoption() {
  const counts = Object.fromEntries(COMPONENTS.map((c) => [c, 0]));
  for (const file of rustFiles(SCREENS)) {
    const text = readFileSync(join(root, file), "utf8");
    for (const c of COMPONENTS) {
      // Only where the name is the `ui` one: screens have their own
      // `Popover`, for one.
      const fromUi = new RegExp(`ui::(\\{[^}]*\\b)?${c}\\b`).test(text);
      if (fromUi) counts[c] += text.match(new RegExp(`\\b${c}::new\\(`, "g"))?.length ?? 0;
    }
  }
  return counts;
}

const dir = mkdtempSync(join(tmpdir(), "design-audit-"));
const rules = [];
try {
  for (const id of RULES) {
    const text = readFileSync(join(root, "test/ast-grep/rules", `${id}.yml`), "utf8");
    const rule = parse(text);
    const debtFiles = notMigrated(text);
    const hits = scan(rule, debtFiles, dir).map((f) => relative(root, join(root, f)));
    const byFile = {};
    for (const f of hits) byFile[f] = (byFile[f] ?? 0) + 1;
    const debt = Object.entries(byFile).filter(([f]) => debtFiles.includes(f));
    rules.push({
      id,
      drift: Object.entries(byFile)
        .filter(([f]) => !debtFiles.includes(f))
        .map(([file, count]) => ({ file, count })),
      debt: debt.reduce((n, [, c]) => n + c, 0),
      debtByFile: Object.fromEntries(debt),
      listed: debtFiles.length,
      stale: debtFiles.filter((f) => !byFile[f]),
    });
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}
const report = { rules, adoption: adoption() };

if (process.argv.includes("--json")) {
  console.log(JSON.stringify(report, null, 2));
} else {
  const lines = [
    "## Code scan",
    "",
    "| Rule | New drift | Debt (matches) | Files not migrated | Stale ignores |",
    "|---|---:|---:|---:|---:|",
    ...rules.map(
      (r) =>
        `| \`${r.id}\` | ${r.drift.reduce((n, d) => n + d.count, 0)} | ${r.debt} | ${r.listed} | ${r.stale.length} |`,
    ),
    "",
  ];
  for (const r of rules.filter((r) => r.drift.length)) {
    lines.push(`New drift in \`${r.id}\`:`, ...r.drift.map((d) => `- ${d.file} (${d.count})`), "");
  }
  for (const r of rules.filter((r) => r.stale.length)) {
    lines.push(
      `\`${r.id}\` ignores files that already pass; remove them:`,
      ...r.stale.map((f) => `- ${f}`),
      "",
    );
  }
  lines.push(
    "## Component adoption",
    "",
    "| Component | Uses in screens |",
    "|---|---:|",
    ...Object.entries(report.adoption).map(([c, n]) => `| \`ui::${c}\` | ${n} |`),
  );
  console.log(lines.join("\n"));
}
process.exitCode = rules.some((r) => r.drift.length) ? 1 : 0;
