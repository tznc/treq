// The sidebar lists stacks whose root has an open (or draft) PR first
// within each repository group.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, ready, settle } from "./helpers.ts";

test("sidebar-open-pr-first: open PRs sort to the top", async ({ page }) => {
  await ready(page);
  // The PR status poll fills each row's PR state.
  await settle(page, 3000);
  await home(page);
  await capture(page, {
    name: "30-sidebar-open-pr-first",
    title: "Sidebar: workspaces with an open PR first",
    expectations: [
      "In each repository group, rows with a green (open) or draft PR icon come before rows without one",
      "Stacked children stay indented directly under their parent",
    ],
  });
});
