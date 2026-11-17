// Bring your own sandbox (BYOS) in Settings → Account, for a Pro and a Free
// user, with no feature preview. Auth, the subscription and every RPC are the
// local Supabase stack (`npm run service-qa:up`). Only the `remote-instance`
// Edge Function is answered from fixtures, by a proxy this spec runs on
// PROXY_PORT: the edge runtime cannot fetch its npm packages in sandboxed CI
// containers. Build the app against the proxy first:
//   set package.json env.dev.supabase.url to http://127.0.0.1:54399 (do not
//   commit), cargo build --features e2e --bin treq, then QA_BYOS_PROXY=1.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { createServer, request, type Server } from "node:http";
import { mkdirSync, openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes, randomUUID } from "node:crypto";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, gitIn, makeFixture, writeFakeAgent } from "../harness.ts";
import { capture, theme } from "./capture.ts";
import { settle } from "./helpers.ts";

const appPort = port + 7;
const PROXY_PORT = 54399;
const SUPABASE = "http://127.0.0.1:54321";
const DB = "postgresql://postgres:postgres@127.0.0.1:54322/postgres";
const ANON = "sb_publishable_ACJWlzQHlZjBrEguHvfOxg_3BJgxAaH";

type Session = { access_token: string; refresh_token: string; user: { id: string } };
type App = { app: ChildProcess; browser: Browser; page: Page };

// What `remote-instance` answers: no workspace yet, one saved Boat key, and
// the BYOS prices for it.
const FIXTURES: Record<string, unknown> = {
  status: { instance: null, endpoint: null },
  list_provider_credentials: {
    credentials: [
      { id: "k1", provider_kind: "boat", label: null, validated_at: "2026-11-01T00:00:00Z", invalid_at: null },
    ],
  },
  byos_catalog: {
    machine_types: [
      { provider_kind: "boat", tier_id: "s", hourly_usd: "0.0009", provider_hourly_usd: "0.018" },
      { provider_kind: "boat", tier_id: "m", hourly_usd: "0.0018", provider_hourly_usd: "0.036" },
    ],
  },
};

function proxy(): Promise<Server> {
  const server = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const body = Buffer.concat(chunks);
      if (req.url?.startsWith("/functions/v1/remote-instance")) {
        const action = JSON.parse(body.toString() || "{}").action as string;
        res.writeHead(action in FIXTURES ? 200 : 400, { "Content-Type": "application/json" });
        res.end(JSON.stringify(FIXTURES[action] ?? { error: `no fixture for ${action}` }));
        return;
      }
      const upstream = request(`${SUPABASE}${req.url}`, { method: req.method, headers: req.headers }, (up) => {
        res.writeHead(up.statusCode ?? 502, up.headers);
        up.pipe(res);
      });
      upstream.on("error", () => res.writeHead(502).end());
      upstream.end(body);
    });
  });
  return new Promise((r) => server.listen(PROXY_PORT, "127.0.0.1", () => r(server)));
}

async function stackUp(): Promise<boolean> {
  try {
    return (await fetch(`${SUPABASE}/auth/v1/health`, { headers: { apikey: ANON } })).ok;
  } catch {
    return false;
  }
}

async function signUp(): Promise<Session> {
  const res = await fetch(`${SUPABASE}/auth/v1/signup`, {
    method: "POST",
    headers: { apikey: ANON, "Content-Type": "application/json" },
    body: JSON.stringify({
      email: `qa-byos-${randomBytes(4).toString("hex")}@example.com`,
      password: `Pw-${randomBytes(9).toString("base64url")}!a1`,
    }),
  });
  expect(res.status).toBe(200);
  return (await res.json()) as Session;
}

/** Gives `userId` an active Pro subscription, as the Stripe webhook would. */
function makePro(userId: string): void {
  const cus = `cus_qa_${randomUUID().slice(0, 8)}`;
  execFileSync("psql", [DB, "-v", "ON_ERROR_STOP=1", "-c", `
    insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
      values ('user', '${userId}', '${cus}');
    insert into public.billing_subscriptions (stripe_subscription_id, stripe_customer_id, owner_type,
      owner_id, plan, status, current_period_end, cancel_at_period_end, stripe_event_created)
      values ('sub_${cus}', '${cus}', 'user', '${userId}', 'pro', 'active', now() + interval '20 days',
        false, now());`]);
}

async function waitForServer(): Promise<void> {
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${appPort}/`)).ok) return;
    } catch {}
    if (Date.now() > deadline) throw new Error("BYOS QA app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
}

async function launch(session: Session): Promise<App> {
  const fixture = makeFixture("treq-qa-byos-");
  const { repo, home } = fixture;
  const git = (...args: string[]) => gitIn(repo, home, ...args);
  git("init", "-b", "main");
  writeFileSync(join(repo, "README.md"), "# byos\n");
  git("add", ".");
  git("commit", "-m", "Initial commit");
  writeFakeAgent(fixture);
  const { DatabaseSync } = await import("node:sqlite");
  const dir = join(home, ".local/share/com.treq");
  mkdirSync(dir, { recursive: true });
  const db = new DatabaseSync(join(dir, "treq.db"));
  db.exec("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
  const set = db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)");
  set.run("theme", theme);
  set.run(
    "supabase_session",
    JSON.stringify({ accessToken: session.access_token, refreshToken: session.refresh_token }),
  );
  db.close();
  const log = openSync(join(fixture.root, "app.log"), "w");
  const app = spawn(appBinary, [], {
    stdio: ["ignore", log, log],
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(appPort),
      TREQ_E2E_REPO: repo,
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
    },
  });
  await waitForServer();
  const browser = await chromium.connect(`ws://127.0.0.1:${appPort}/`);
  const page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 1000 });
  return { app, browser, page };
}

async function stop({ app, browser }: App): Promise<void> {
  await browser.close().catch(() => {});
  const exited = new Promise((r) => app.once("exit", r));
  app.kill("SIGTERM");
  await Promise.race([exited, new Promise((r) => setTimeout(r, 10_000))]);
  if (app.exitCode === null && app.signalCode === null) app.kill("SIGKILL");
}

async function openAccount(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Settings", exact: true }).click({ timeout: 30_000 });
  await page.getByRole("tab", { name: "Account" }).click();
  await expect(page.getByRole("button", { name: "Sign Out" })).toBeVisible({ timeout: 30_000 });
  await page.mouse.move(1300, 980);
  await settle(page, 2500);
}

/** Scrolls the settings page until `locator` is drawn (GPUI draws only what is on screen). */
async function scrollTo(page: Page, locator: ReturnType<Page["getByTestId"]>): Promise<void> {
  await page.mouse.move(950, 500);
  for (let i = 0; i < 25 && (await locator.count()) === 0; i++) {
    await page.mouse.wheel(0, 300);
    await settle(page, 400);
  }
}

let server: Server | undefined;
test.beforeAll(async () => {
  if (process.env.QA_BYOS_PROXY === "1" && (await stackUp())) server = await proxy();
});
test.afterAll(async () => {
  await new Promise((r) => (server ? server.close(r) : r(null)));
});

test.describe.configure({ mode: "serial" });

test("byos: Pro creates on their own account from Provider keys; the card names no provider", async () => {
  test.skip(!server, "needs QA_BYOS_PROXY=1, the proxy build and the local Supabase stack");
  const session = await signUp();
  makePro(session.user.id);
  const run = await launch(session);
  const { page } = run;
  try {
    await openAccount(page);
    // The section's container is not observable; its buttons are.
    const addDaytona = page.getByRole("button", { name: "Add Daytona key" });
    await scrollTo(page, addDaytona as never);
    await expect(addDaytona).toBeVisible({ timeout: 30_000 });
    await expect(page.getByRole("button", { name: "Add E2B key" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Add Fly Machines key" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Create workspace on this account" })).toBeVisible();
    await page.mouse.wheel(0, 3000);
    await settle(page, 1200);
    await capture(page, {
      name: "22-byos-01-provider-keys",
      title: "Pro, no feature preview: the Provider keys section",
      expectations: [
        "A 'Provider keys / Bring your own sandbox' section says treq charges 5% of the provider's list price",
        "The saved Boat key reads 'Key saved' with 'Create workspace on this account' and Remove",
        "Add Daytona key and Add E2B key buttons show, and no Fly Machines button",
      ],
    });

    await page.mouse.wheel(0, -6000);
    await settle(page, 1200);
    await expect(page.getByRole("button", { name: "Create cloud workspace" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Your Boat account" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "treq managed" })).toHaveCount(0);
    await capture(page, {
      name: "22-byos-02-managed-card",
      title: "Cloud workspace card: treq managed only",
      expectations: [
        "The cloud workspace card shows sizes and 'Create cloud workspace' only",
        "No provider name and no 'treq managed' / 'Your … account' choice appears on the card",
      ],
    });

    await page.getByRole("tab", { name: "Feature Preview" }).click();
    await page.mouse.move(1300, 980);
    await settle(page, 1200);
    await capture(page, {
      name: "22-byos-03-feature-previews",
      title: "Feature Preview tab",
      expectations: ["No 'Bring your own sandbox' row among the previews"],
    });
  } finally {
    await stop(run);
  }
});

test("byos: hidden for a Free user", async () => {
  test.skip(!server, "needs QA_BYOS_PROXY=1, the proxy build and the local Supabase stack");
  const run = await launch(await signUp());
  const { page } = run;
  try {
    await openAccount(page);
    await scrollTo(page, page.getByRole("button", { name: "Delete Account" }) as never);
    await expect(page.getByRole("button", { name: "Add Daytona key" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Your Boat account" })).toHaveCount(0);
    await capture(page, {
      name: "22-byos-04-free-user",
      title: "Free user: no BYOS",
      expectations: [
        "The Account tab shows the signed-in Free user with an upgrade path",
        "Scrolled to the bottom: no Provider keys section",
      ],
    });
  } finally {
    await stop(run);
  }
});
