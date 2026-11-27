// Editing diff lines in place: select lines, E opens an editor over them,
// Ctrl+Enter writes the file. Runs on the alerting workspace's uncommitted
// edits (qa/launch.ts).
import { readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const ALERTING = "feat/alerting-logs";
const FILE = "packages/api/src/alerting.ts";

/** The checkout of the running QA app's alerting workspace. */
function alertingDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => statSync(join(d, "qa-extra.json"), { throwIfNoEntry: false })?.isFile() ?? false)
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const root = roots[0];
  if (!root) throw new Error("no QA fixture");
  const { repo } = JSON.parse(readFileSync(join(root, "qa-extra.json"), "utf8"));
  return workspaceDir(repo, ALERTING);
}

test("diff line edit: E edits the selected lines and saves them to the file", async ({ page }) => {
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await settle(page, 1500);

  // The gutter of formatError's first line, then Shift+Down to take the next.
  const row = page.getByRole("row", { name: "+export function formatError" });
  const box = await row.boundingBox();
  if (!box) throw new Error("formatError row not drawn");
  await page.mouse.click(box.x + 30, box.y + box.height / 2);
  await page.keyboard.press("Shift+ArrowDown");
  await page.keyboard.press("e");
  const editor = page.getByRole("textbox", { name: "Edit lines" });
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue(
    'export function formatError(line: string): string {\n  return formatAlert(line, "error");',
  );
  await settle(page, 400);
  await capture(page, {
    name: "21-line-edit-01-editor",
    title: "E opens an editor over the selected lines",
    expectations: [
      "The two formatError lines are replaced by a monospace editor holding them, aligned with the code column",
      "The lines above and below the editor are drawn as before",
      "Cancel and Save buttons sit under the editor on the right",
    ],
  });

  await editor.fill(
    'export function formatError(line: string, code = 500): string {\n  return formatAlert(line, `error:${code}`);',
  );
  await page.keyboard.press("Control+Enter");
  await expect(editor).toHaveCount(0);
  await settle(page, 2000);
  expect(readFileSync(join(alertingDir(), FILE), "utf8")).toContain(
    "export function formatError(line: string, code = 500): string {\n  return formatAlert(line, `error:${code}`);\n}",
  );
  await capture(page, {
    name: "21-line-edit-02-saved",
    title: "Ctrl+Enter writes the lines and the diff shows them",
    expectations: [
      "The editor is gone and alerting.ts shows the edited formatError lines as added lines",
      "The rest of the diff is unchanged",
    ],
  });
});
