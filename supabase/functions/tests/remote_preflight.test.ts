// Unit tests for the provider-switch preflight: the script's arguments and
// the parser that turns its output into a per-repo report. No network.
//
// Run with: `deno test supabase/functions/tests/remote_preflight.test.ts`

import { assert, assertEquals, assertFalse } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  parsePreflightOutput,
  preflightCommand,
  recloneCommand,
} from "../_shared/remote/preflight.ts";

Deno.test("the preflight command passes each repo path as its own argument", () => {
  const cmd = preflightCommand(["/home/treq/a", "/home/treq/b c"]);
  assertEquals(cmd[0], "bash");
  assertEquals(cmd[1], "-c");
  assertEquals(cmd.slice(3), ["preflight", "/home/treq/a", "/home/treq/b c"]);
});

Deno.test("clean repos need no acknowledgement", () => {
  const out = [
    "REPO\t/home/treq/a",
    "VCS\tjj",
    "DIRTY\t0",
    "UNPUSHED\t0",
    "AGENTS\t0",
    "REPO\t/home/treq/b",
    "VCS\tgit",
    "DIRTY\t0",
    "UNPUSHED\t0",
    "AGENTS\t0",
    "",
  ].join("\n");
  const report = parsePreflightOutput(out, ["/home/treq/a", "/home/treq/b"]);
  assertFalse(report.requires_ack);
  assertEquals(report.repos.length, 2);
  assertEquals(report.repos[0], {
    path: "/home/treq/a",
    vcs: "jj",
    dirty: 0,
    unpushed: 0,
    agents: 0,
    error: null,
  });
});

Deno.test("dirty, unpushed or running agents each require acknowledgement", () => {
  for (const line of ["DIRTY\t2", "UNPUSHED\t1", "AGENTS\t3"]) {
    const base = ["REPO\t/r", "VCS\tjj", "DIRTY\t0", "UNPUSHED\t0", "AGENTS\t0"];
    const out = base.map((l) => (l.split("\t")[0] === line.split("\t")[0] ? line : l)).join("\n");
    const report = parsePreflightOutput(out, ["/r"]);
    assert(report.requires_ack, line);
  }
});

Deno.test("a repo with no output is reported as an error and requires acknowledgement", () => {
  const report = parsePreflightOutput("REPO\t/a\nVCS\tgit\nDIRTY\t0\nUNPUSHED\t0\nAGENTS\t0\n", ["/a", "/b"]);
  assertEquals(report.repos[1].path, "/b");
  assertEquals(report.repos[1].error, "no preflight output");
  assert(report.requires_ack);
});

Deno.test("a missing repo directory is reported, not counted as lost work", () => {
  const report = parsePreflightOutput("REPO\t/a\nMISSING\n", ["/a"]);
  assertEquals(report.repos[0].vcs, "none");
  assertEquals(report.repos[0].error, "missing");
  assertFalse(report.requires_ack);
});

Deno.test("script errors and unparseable counts require acknowledgement", () => {
  const err = parsePreflightOutput("REPO\t/a\nVCS\tjj\nERROR\tjj log failed\n", ["/a"]);
  assertEquals(err.repos[0].error, "jj log failed");
  assert(err.requires_ack);
  const junk = parsePreflightOutput("REPO\t/a\nVCS\tgit\nDIRTY\tlots\nUNPUSHED\t0\nAGENTS\t0\n", ["/a"]);
  assert(junk.requires_ack);
});

Deno.test("no repos means nothing to lose", () => {
  const report = parsePreflightOutput("", []);
  assertEquals(report.repos, []);
  assertFalse(report.requires_ack);
});

Deno.test("output for repos that were not asked about is ignored", () => {
  const report = parsePreflightOutput("REPO\t/other\nDIRTY\t5\n", []);
  assertEquals(report.repos, []);
  assertFalse(report.requires_ack);
});

Deno.test("the re-clone command passes url and path as arguments", () => {
  const cmd = recloneCommand("https://github.com/o/r.git", "/home/treq/r");
  assertEquals(cmd.slice(3), ["reclone", "https://github.com/o/r.git", "/home/treq/r"]);
});
