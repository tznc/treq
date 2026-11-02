// The Changes tab on a working copy with 1,500 new files: the file list is a
// virtual list, so only the rows on screen are built. Scrolls the list to
// its end, picks a row there and checks the diff follows. Removes the files
// when done.
import { mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { CACHE, workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const FILES = 1500;

/** The checkout of the running QA app's cache workspace. */
function cacheDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const { repo } = JSON.parse(readFileSync(join(roots[0], "qa-extra.json"), "utf8"));
  return workspaceDir(repo, CACHE);
}

const name = (i: number) => `file${String(i).padStart(4, "0")}.ts`;

test("changes-many-files: a long file list scrolls and picks a file", async ({ page }) => {
  test.setTimeout(600_000);
  const dir = cacheDir();
  mkdirSync(join(dir, "src/many"), { recursive: true });
  for (let i = 0; i < FILES; i++) {
    writeFileSync(join(dir, "src/many", name(i)), `export const value${i} = ${i};\n`);
  }
  try {
    await openWorkspace(page, CACHE);
    await openTab(page, "Changes");
    await settle(page, 4000);
    await capture(page, {
      name: "22-changes-many-files-01-top",
      title: "Changes tab with 1,500 new files, list at the top",
      expectations: [
        "Sidebar: a Changes header with a count near 1,500, the commit box under it, then file rows",
        "The diff shows the first files' cards on the right",
      ],
    });
    // Scroll the sidebar list to its end.
    await page.mouse.move(330, 500);
    // The virtual list moves as far as the rows measured so far reach per
    // wheel step, so it takes a few dozen steps to get to the end.
    for (let i = 0; i < 80; i++) {
      await page.mouse.wheel(0, 100_000);
      await page.waitForTimeout(60);
    }
    await settle(page, 800);
    await capture(page, {
      name: "22-changes-many-files-02-end",
      title: "File list scrolled to its last rows",
      expectations: [
        "Sidebar rows end with the last files (file14xx.ts), the list is not blank",
        "Row text, status letters and spacing match the top of the list",
      ],
    });
    // Pick the row under the pointer: the diff scrolls to that file.
    await page.mouse.click(330, 400);
    await settle(page, 1500);
    await capture(page, {
      name: "22-changes-many-files-03-picked",
      title: "A file picked far down the list",
      expectations: [
        "The picked row is highlighted in the sidebar",
        "The diff shows that file's card at the top of the diff area",
      ],
    });
  } finally {
    rmSync(join(dir, "src/many"), { recursive: true, force: true });
  }
});
