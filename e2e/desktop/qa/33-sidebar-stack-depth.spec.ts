// Deep stacks in the sidebar: a stack four levels deep, made through the
// New workspace dialog's indented lines. The first stacked level hangs from
// its root by an elbow; every deeper row keeps that one indent and joins the
// row above by a straight line. A second child stacked mid-stack shows how a
// branch reads at one indent.
import { expect, type Page, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openWorkspace, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const FIRST = "feat/empty-event-message";
const DEEP = ["qa/deep-two", "qa/deep-three", "qa/deep-four"] as const;
const SIDE = "qa/deep-side";

const heading = (p: Page) => p.getByRole("heading", { name: "New workspace" });
const title = (p: Page, branch: string) =>
  row(p, branch).getByTestId(/^workspace-\d+-title$/);

/** Stacks `lines` (an indented line stacks on the one above) on `base`. */
async function stackOn(page: Page, base: string, lines: string): Promise<void> {
  await row(page, base).hover();
  await row(page, base).getByRole("button", { name: "Stack on this…" }).click();
  await expect(heading(page)).toBeVisible();
  await page.getByLabel("Workspaces", { exact: true }).fill(lines);
  await settle(page, 1200); // the line check is debounced
  await page
    .getByRole("button", { name: /^Create/ })
    .last()
    .click();
  await expect(heading(page)).toHaveCount(0, { timeout: 60_000 });
}

/** The sidebar alone, at `zoom` times its width. */
const sidebar = (zoom = 1) => ({ x: 0, y: 0, width: 280 * zoom, height: 900 });

test("sidebar-stack-depth: deep rows keep the first level's indent", async ({
  page,
}) => {
  await ready(page);
  await openWorkspace(page, FIRST);
  await stackOn(page, FIRST, `${DEEP[0]}\n\t${DEEP[1]}\n\t${DEEP[2]}`);
  await expect(row(page, DEEP[2])).toBeVisible({ timeout: 60_000 });
  await settle(page, 1500);
  await capture(page, {
    name: "33-sidebar-stack-depth",
    title: "A stack four levels deep",
    clip: sidebar(),
    expectations: [
      `${FIRST} hangs from feat/event-ingest by an elbow`,
      `${DEEP.join(", ")} sit at ${FIRST}'s indent, with no elbows`,
      "A straight line joins each of those icons to the icon above it",
    ],
  });
  const left = (await title(page, FIRST).boundingBox())?.x;
  for (const branch of DEEP) {
    expect((await title(page, branch).boundingBox())?.x).toBe(left);
    await expect(row(page, branch).getByTestId(/-line$/)).toHaveCount(1);
    await expect(row(page, branch).getByTestId(/-elbow$/)).toHaveCount(0);
  }
  await expect(row(page, FIRST).getByTestId(/-elbow$/)).toHaveCount(1);
});

test("sidebar-stack-depth: the deep stack at 150% zoom", async ({ page }) => {
  await ready(page);
  try {
    for (let i = 0; i < 10; i++) await page.keyboard.press(`${mod}+=`);
    await settle(page, 1500);
    await capture(page, {
      name: "33-sidebar-stack-depth-zoom-150",
      title: "The deep stack at 150% zoom",
      clip: sidebar(1.5),
      expectations: [
        "The deep rows still share the first level's indent",
        "Each straight line still runs from the icon above into the row's own icon",
      ],
    });
  } finally {
    for (let i = 0; i < 10; i++) await page.keyboard.press(`${mod}+-`);
    await settle(page, 500);
  }
});

test("sidebar-stack-depth: a branched stack at one indent", async ({
  page,
}) => {
  await ready(page);
  await stackOn(page, DEEP[0], SIDE);
  await expect(row(page, SIDE)).toBeVisible({ timeout: 60_000 });
  await settle(page, 1500);
  await capture(page, {
    name: "33-sidebar-stack-depth-branched",
    title: `A second child of ${DEEP[0]}`,
    clip: sidebar(),
    expectations: [
      `${SIDE} sits at the same indent as the rest of the stack`,
      "It joins the row above it by a straight line, like every deep row",
    ],
    issue: `Siblings read as one chain: nothing shows ${SIDE} and ${DEEP[1]} share ${DEEP[0]} as parent`,
  });
  expect((await title(page, SIDE).boundingBox())?.x).toBe(
    (await title(page, FIRST).boundingBox())?.x,
  );
});
