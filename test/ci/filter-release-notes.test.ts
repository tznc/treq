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

  it("drops ci, test, build and refactor entries, scoped or not", () => {
    const notes = [
      "* refactor(ui): split sidebar by @a in https://x/pull/13",
      "* refactor: inline helper by @a in https://x/pull/14",
      "* ci: cache cargo by @a in https://x/pull/10",
      "* test(lint): require testable node by @a in https://x/pull/11",
      "* build(deps)!: bump tauri by @a in https://x/pull/12",
      "* feat(sidebar): show PR state pill by @a in https://x/pull/1",
      "",
    ].join("\n");
    expect(filter(notes).stdout).toBe(
      "* feat(sidebar): show PR state pill by @a in https://x/pull/1\n",
    );
  });

  it("drops only the listed feat and fix scopes", () => {
    const notes = [
      "* feat(web): new landing page by @a in https://x/pull/20",
      "* fix(lint): flag raw colors by @a in https://x/pull/21",
      "* feat(lint): require 13px minimum by @a in https://x/pull/25",
      "* fix(deps): bump serde by @a in https://x/pull/26",
      "* fix(e2e): wait for sidebar by @a in https://x/pull/27",
      "* fix(test): stabilise clock by @a in https://x/pull/28",
      "* fix(web): broken docs link by @a in https://x/pull/29",
      "* feat(web)!: drop legacy docs by @a in https://x/pull/22",
      "* feat(webview): open links by @a in https://x/pull/23",
      "* feat: add login by @a in https://x/pull/24",
      "* fix(app): match button height by @a in https://x/pull/6",
      "",
    ].join("\n");
    expect(filter(notes).stdout).toBe(
      [
        "* feat(webview): open links by @a in https://x/pull/23",
        "* feat: add login by @a in https://x/pull/24",
        "* fix(app): match button height by @a in https://x/pull/6",
        "",
      ].join("\n"),
    );
  });

  it("keeps entries that only mention the words later in the title", () => {
    const line =
      "* fix(app): style chore ci test build refactor runner by @a in https://x/pull/7\n";
    expect(filter(line).stdout).toBe(line);
  });

  it("succeeds when every entry is dropped", () => {
    const r = filter("* chore: bump by @a in https://x/pull/8\n");
    expect(r.status).toBe(0);
    expect(r.stdout).toBe("");
  });
});
