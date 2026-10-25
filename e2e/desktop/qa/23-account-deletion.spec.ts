// Deleting the Treq account from Settings → Account: the danger row at the
// bottom of the tab, the dialog that wants the email typed, the server's
// refusal shown in it, and the signed-out tab with its toast after a
// deletion. Needs the app signed in against the fake Supabase
// (`QA_ACCOUNT=signed-in`, see launch.ts); skipped otherwise.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { QA_USER, SOLE_OWNER } from "./fake-supabase.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });
test.skip(process.env.QA_ACCOUNT !== "signed-in", "needs QA_ACCOUNT=signed-in");

async function accountTab(page: Page): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: "Account" }).click();
  await expect(page.getByRole("tab", { name: "Account", selected: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign Out" })).toBeVisible();
  await page.mouse.move(950, 500);
  await page.mouse.wheel(0, 2000);
  await settle(page, 1200);
}

test("account: delete the account", async ({ page }) => {
  await accountTab(page);
  await capture(page, {
    name: "23-account-signed-in",
    title: "Settings: Account tab, signed in, scrolled to the end",
    expectations: [
      "Profile, subscription and cloud workspace cards, then Sign Out",
      "A bordered Delete account row closes the tab, its button in red text",
    ],
  });

  const open = page.getByRole("button", { name: "Delete Account…" });
  await expect(open).toBeVisible();
  await open.click();
  await settle(page, 800);
  const confirm = page.getByRole("button", { name: "Delete Account", exact: true });
  await expect(confirm).toBeDisabled();
  await capture(page, {
    name: "23-account-delete-dialog",
    title: "Delete account dialog, nothing typed",
    expectations: [
      "Title asks to delete the Treq account; two plain sentences say what goes and what stays",
      "The email field asks for ada@example.com; Delete Account is disabled",
    ],
  });

  await page.getByRole("textbox", { name: QA_USER.email }).fill(QA_USER.email);
  await expect(confirm).toBeEnabled();
  await confirm.click();
  // Plain text is not observable: the refusal leaves the dialog open with
  // the button usable again, and the capture shows the sentence.
  await settle(page, 1200);
  await expect(confirm).toBeEnabled();
  await capture(page, {
    name: "23-account-delete-refused",
    title: "Delete account dialog after the server refused",
    expectations: [
      `A red alert says the account was not deleted: "${SOLE_OWNER}"`,
      "The dialog stays open with the email still typed, Cancel and Delete Account usable",
    ],
  });

  await confirm.click();
  await expect(page.getByRole("button", { name: "Sign in with Browser" })).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "23-account-deleted",
    title: "After deleting: signed out, with a toast",
    screen: true,
    expectations: [
      "The Account tab shows Sign in to Treq; the dialog is gone",
      "A success toast says Account deleted and that local repositories are unchanged",
    ],
  });
  await page.getByRole("button", { name: "Close", exact: true }).click();
});
