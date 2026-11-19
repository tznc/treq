// The workspace header's Create PR split button and Merge… share one height.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, openWorkspace, ready, settle } from "./helpers.ts";

const overrides = () => join(fixture().home, "fake-gh-prs.json");

test.afterAll(() => rmSync(overrides(), { force: true }));

test("header-button-heights: Create PR matches Merge…", async ({ page }) => {
  test.setTimeout(180_000);
  await ready(page);
  // A closed PR may be replaced, so the header offers Create PR.
  writeFileSync(overrides(), JSON.stringify({ "feat/keyvalues-cache": { state: "CLOSED" } }));
  await home(page);
  await settle(page, 25_000);
  await openWorkspace(page, "feat/keyvalues-cache");
  await settle(page, 2500);
  const create = page.getByRole("button", { name: "Create PR" });
  const merge = page.getByRole("button", { name: "Merge…" });
  await expect(create).toBeVisible();
  await expect(merge).toBeVisible();
  const [c, m] = [await create.boundingBox(), await merge.boundingBox()];
  expect(c!.height).toBe(m!.height);
  expect(c!.y).toBe(m!.y);
  await capture(page, {
    name: "31-header-button-heights",
    title: "Create PR and Merge… in the workspace header",
    expectations: [
      "Create PR split button and Merge… are the same height",
      "Their tops and bottoms line up on one row",
      "Create PR label and chevron are centred, unclipped",
    ],
  });
});
