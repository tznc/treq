// Starts the mobile app's desktop build on a fresh home and connects
// Playwright to it, as qa/18-ssh-remote.spec.ts starts the desktop app.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Browser, type Page } from "@playwright/test";
import { appEnv, connectApp, makeFixture, serverUp } from "../harness.ts";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../..");

/** `cargo build -p treq --example mobile --features mobile-e2e`. */
export const mobileBinary =
  process.env.TREQ_MOBILE_BIN ??
  join(process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target"), "debug/examples/mobile");

export const mobilePort = Number(process.env.TREQ_MOBILE_PORT ?? 9353);

/** The SSH host `qa/ssh-server/up.sh` prints (with QA_SSH_FAKE_GH=1). */
export const host = {
  host: process.env.QA_SSH_HOST ?? "",
  port: process.env.QA_SSH_PORT ?? "22",
  user: process.env.QA_SSH_USER ?? "",
  key: process.env.QA_SSH_KEY ?? "",
  repo: process.env.QA_SSH_REPO ?? "",
};

export type MobileApp = { app: ChildProcess; browser: Browser; page: Page; home: string };

/** A fresh home whose ~/.ssh holds the key the host trusts, and the app on it. */
export async function launchMobile(): Promise<MobileApp> {
  const fixture = makeFixture("treq-qa-mobile-");
  if (host.key) {
    const ssh = join(fixture.home, ".ssh");
    mkdirSync(ssh, { recursive: true, mode: 0o700 });
    copyFileSync(host.key, join(ssh, "id_ed25519"));
    copyFileSync(`${host.key}.pub`, join(ssh, "id_ed25519.pub"));
  }
  const app = spawn(mobileBinary, [], {
    stdio: process.env.QA_DEBUG ? "inherit" : "ignore",
    env: { ...appEnv(fixture), GPUI_PLAYWRIGHT: String(mobilePort) },
  });
  await serverUp(app, mobilePort);
  const { browser, page } = await connectApp(mobilePort);
  // A phone, as the example's window is: one pane, not the tablet split.
  await page.setViewportSize({ width: 420, height: 860 });
  return { app, browser, page, home: fixture.home };
}

/** Runs `script` on the host, outside the app. */
export function onHost(script: string): string {
  return execFileSync(
    "ssh",
    [
      "-i", host.key, "-p", host.port,
      "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
      `${host.user}@${host.host}`, script,
    ],
    { encoding: "utf8" },
  );
}

/**
 * Replaces QA_SSH_REPO with a fresh Git repository whose origin is the
 * repository the host's fake `gh` answers for, and gives #418 a diff.
 */
export function seedRepo(): void {
  onHost(
    `set -e; rm -rf '${host.repo}'; mkdir -p '${host.repo}'; cd '${host.repo}'
     git init -q -b main
     git config user.name QA; git config user.email qa@example.com
     printf '# event-bus\\n' > README.md
     git add .; git commit -qm 'Initial commit'
     git remote add origin https://github.com/lorem-ipsum/event-bus.git
     cat > ~/fake-gh-pr-diff-418.diff <<'DIFF'
${PR_418_DIFF}DIFF`,
  );
}

/** What the fake `gh` serves as #418's diff (`gh pr diff 418`). */
const PR_418_DIFF = `diff --git a/web/src/events/post.ts b/web/src/events/post.ts
index 1111111..2222222 100644
--- a/web/src/events/post.ts
+++ b/web/src/events/post.ts
@@ -1,5 +1,7 @@
 import type { EventBody } from "../../../api/src/schema";
 
 export async function postEvent(body: EventBody): Promise<Response> {
-  const message = body.event_message;
+  const message =
+    typeof body.event_message === "string" && body.event_message.length > 0
+      ? body.event_message
+      : JSON.stringify(body);
   return fetch("/events", { method: "POST", body: message });
 }
`;

/** Adds the host through the add-host form and trusts its key, as a user does. */
export async function addHost(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Use your own SSH host" }).click();
  await page.getByRole("textbox", { name: "dev@build.lan:2222" }).fill(`${host.user}@${host.host}:${host.port}`);
  await page.getByRole("textbox", { name: "Name (e.g. Workstation)" }).fill("QA host");
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await page.getByRole("button", { name: "Trust and connect" }).click();
  await expect(page.getByRole("textbox", { name: "~/src/my-repo" })).toBeVisible();
}

/** Opens QA_SSH_REPO by path, then its GitHub page from the ⋯ menu. */
export async function openGithub(page: Page): Promise<void> {
  await page.getByRole("textbox", { name: "~/src/my-repo" }).fill(host.repo);
  await page.getByRole("button", { name: "Inspect repository" }).click();
  await expect(page.getByRole("button", { name: "New workspaces" })).toBeVisible();
  await page.getByRole("button", { name: "More" }).click();
  await page.getByRole("menuitem", { name: "GitHub" }).click();
  await expect(page.getByRole("tab", { name: "Issues" })).toBeVisible();
}
