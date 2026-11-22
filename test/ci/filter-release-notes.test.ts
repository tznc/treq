import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SCRIPT = join(
  import.meta.dirname,
  "../../scripts/filter-release-notes.sh",
);

const filter = (input: string) =>
  spawnSync("bash", [SCRIPT], { input, encoding: "utf8" });

describe("filter-release-notes.sh", () => {
  it("drops chore, lint and style entries, scoped or not", () => {
    const notes = [
      "## What's Changed",
      "* feat(sidebar): show PR state pill by @a in https://x/pull/1",
      "* chore: bump version to v0.4.2 by @a in https://x/pull/2",
      "* lint(ui): require 13px minimum by @a in https://x/pull/3",
      "* style(changes): review thread header by @a in https://x/pull/4",
      "* chore(deps)!: drop node 18 by @a in https://x/pull/5",
      "* fix(app): match button height by @a in https://x/pull/6",
      "",
      "**Full Changelog**: https://x/compare/v0.4.1...v0.4.2",
      "",
    ].join("\n");

    const r = filter(notes);

    expect(r.status).toBe(0);
    expect(r.stdout).toBe(
      [
        "## What's Changed",
        "* feat(sidebar): show PR state pill by @a in https://x/pull/1",
        "* fix(app): match button height by @a in https://x/pull/6",
        "",
        "**Full Changelog**: https://x/compare/v0.4.1...v0.4.2",
        "",
      ].join("\n"),
    );
  });

  it("keeps entries that only mention the words later in the title", () => {
    const line = "* fix(lint): style chore runner by @a in https://x/pull/7\n";
    expect(filter(line).stdout).toBe(line);
  });

  it("succeeds when every entry is dropped", () => {
    const r = filter("* chore: bump by @a in https://x/pull/8\n");
    expect(r.status).toBe(0);
    expect(r.stdout).toBe("");
  });
});
