// Pasting into the terminal pane: text goes in as before, and a copied
// image goes in as the path of a temp file holding it (Claude Code and
// Codex attach a pasted image path). The X clipboard is set with xclip,
// which stays behind to serve it, so its output is not waited on.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";
const IMAGE = join(
  dirname(fileURLToPath(import.meta.url)),
  "../../../assets/combined-horizontal.png",
);

function copyImage(): void {
  execFileSync("xclip", ["-selection", "clipboard", "-t", "image/png", "-i", IMAGE], {
    stdio: "ignore",
  });
}

function copyText(text: string): void {
  execFileSync("xclip", ["-selection", "clipboard"], {
    input: text,
    stdio: ["pipe", "ignore", "ignore"],
  });
}

test.describe.configure({ mode: "serial" });

test("terminal paste: text and clipboard images", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type("PS1='shell $ '; clear\n");
  await settle(page, 800);

  copyText("echo text-paste-still-works");
  await page.keyboard.press("Control+Shift+V");
  await page.keyboard.press("Enter");
  await settle(page, 1000);
  await capture(page, {
    name: "21-terminal-paste-text",
    title: "Shell: text paste",
    expectations: [
      "The shell shows the pasted command 'echo text-paste-still-works' and its output line 'text-paste-still-works'",
    ],
  });

  copyImage();
  await page.keyboard.type("ls -l ");
  await page.keyboard.press("Control+Shift+V");
  await settle(page, 1000);
  await capture(page, {
    name: "21-terminal-paste-image-shell",
    title: "Shell: a copied image pasted at the prompt",
    expectations: [
      "After 'ls -l ' the prompt holds a quoted path ending in treq-pasted-images/paste-<uuid>.png",
      "No garbled escape sequences or broken image show in the shell",
    ],
  });
  await page.keyboard.press("Enter");
  await settle(page, 1000);
  await capture(page, {
    name: "21-terminal-paste-image-shell-file",
    title: "Shell: the pasted image's file exists",
    expectations: [
      "ls -l lists the pasted file: a non-zero size and the same paste-<uuid>.png path, so the image was saved",
    ],
  });

  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await page.mouse.click(1000, 780);
  copyImage();
  await page.keyboard.press("Control+Shift+V");
  await settle(page, 1000);
  await capture(page, {
    name: "21-terminal-paste-image-agent",
    title: "Agent: a copied image pasted into the session",
    expectations: [
      "The agent column shows its transcript, then the pasted quoted path ending in treq-pasted-images/paste-<uuid>.png",
      "The shell column beside it still shows the earlier ls -l output",
    ],
  });
});
