// Starts the mobile app's desktop build on a fresh home and connects
// Playwright to it, as qa/18-ssh-remote.spec.ts starts the desktop app.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, expect, type Browser, type Page } from "@playwright/test";
import { appEnv, makeFixture } from "../harness.ts";

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
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${mobilePort}/`)).ok) break;
    } catch {}
    if (Date.now() > deadline) throw new Error("the mobile app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
  let browser: Browser | undefined;
  for (let attempt = 1; ; attempt++) {
    try {
      browser = await chromium.connect(`ws://127.0.0.1:${mobilePort}/`);
      const page = await (await browser.newContext()).newPage();
      // A phone, as the example's window is: one pane, not the tablet split.
      await page.setViewportSize({ width: 420, height: 860 });
      return { app, browser, page, home: fixture.home };
    } catch (error) {
      await browser?.close().catch(() => {});
      if (attempt >= 10) throw error;
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
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
 * repository the host's fake `gh` answers for.
 */
export function seedRepo(): void {
  onHost(
    `set -e; rm -rf '${host.repo}'; mkdir -p '${host.repo}'; cd '${host.repo}'
     git init -q -b main
     git config user.name QA; git config user.email qa@example.com
     printf '# event-bus\\n' > README.md
     git add .; git commit -qm 'Initial commit'
     git remote add origin https://github.com/lorem-ipsum/event-bus.git`,
  );
}

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
