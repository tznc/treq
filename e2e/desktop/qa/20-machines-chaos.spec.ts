// Chaos on the machines screens (the Overview's machine list, the Treq Cloud
// setup dialog and the "Your own VM" form) in the real app. A second app
// starts on a fresh home with an SSH key and a stored session, and talks to
// a fake Supabase on the dev URL (127.0.0.1:54321, package.json's dev env)
// whose answers the test switches: ok (the workspace stays provisioning),
// 500, 401, malformed JSON, a reset connection, nothing listening, and a
// black hole that accepts and never answers. Then targeted scenarios (form
// names, double-clicks, closing and navigating mid-provision, tiny and huge
// windows, zoom extremes) and a seeded monkey limited to these screens.
// After every step the process must be alive, Playwright must answer and the
// app log must have no `panicked`. `SEED` and `CHAOS_STEPS` vary the monkey;
// `HANG_WAIT` (ms) how long the black-hole scenario waits for a timeout.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { createServer, type Server, type IncomingMessage, type ServerResponse } from "node:http";
import { createServer as netServer, type Server as NetServer, type Socket } from "node:net";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, makeFixture } from "../harness.ts";
import { capture, theme } from "./capture.ts";

const appPort = port + 6;
const SUPABASE_PORT = 54321;
const STEPS = Number(process.env.CHAOS_STEPS ?? 80);
const HANG_WAIT = Number(process.env.HANG_WAIT ?? 175_000);
let seed = Number(process.env.SEED ?? 20);
const seed0 = seed;

type Mode = "ok" | "e500" | "e401" | "malformed" | "reset" | "hang" | "down";

// -- The fake Supabase --------------------------------------------------------

const fake = {
  mode: "ok" as Mode,
  /** Set by `ensure`: the workspace then stays `provisioning`. */
  instance: null as null | { status: string },
  calls: [] as string[],
  hung: [] as ServerResponse[],
};
let server: Server | null = null;
const sockets = new Set<Socket>();

function json(res: ServerResponse, status: number, body: unknown) {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}

async function handle(req: IncomingMessage, res: ServerResponse, body: string) {
  const url = req.url ?? "";
  let action = "";
  try {
    action = JSON.parse(body || "{}").action ?? "";
  } catch {}
  fake.calls.push(`${fake.mode} ${req.method} ${url.split("?")[0]} ${action}`);
  // Signing in keeps working in every mode: the modes are about the cloud.
  if (url.startsWith("/auth/v1/token")) {
    return json(res, 200, {
      access_token: "a1",
      refresh_token: "r1",
      expires_in: 3600,
      token_type: "bearer",
      user: { id: "u1", email: "qa@example.com", user_metadata: { full_name: "QA Chaos" } },
    });
  }
  if (url.startsWith("/rest/v1/subscriptions")) {
    return json(res, 200, [{ status: "active", plan: "pro", current_period_end: null }]);
  }
  if (!url.startsWith("/functions/v1/remote-")) return json(res, 404, { error: "not found" });
  switch (fake.mode) {
    case "e500":
      return json(res, 500, { error: "internal", message: "Edge Function crashed (QA 500)" });
    case "e401":
      return json(res, 401, { code: "unauthorized", message: "Invalid JWT" });
    case "malformed":
      res.writeHead(200, { "content-type": "application/json" });
      return res.end('{"instance": {"status": "rea');
    case "reset":
      return req.socket.destroy();
    case "hang":
      fake.hung.push(res);
      return;
  }
  if (url.startsWith("/functions/v1/remote-ssh-trust")) {
    return json(res, 200, { key: { id: "k1", comment: "treq-managed-vm" } });
  }
  if (action === "ensure") {
    fake.instance = { status: "provisioning" };
    return json(res, 200, {});
  }
  if (action === "status") {
    return json(res, 200, {
      instance: fake.instance && {
        instance_id: "i1",
        status: fake.instance.status,
        generation: 1,
        disk_quota_gb: 10,
      },
      endpoint: null,
    });
  }
  return json(res, 200, {});
}

async function startFake(): Promise<void> {
  if (server) return;
  server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => void handle(req, res, body));
  });
  server.on("connection", (s) => {
    sockets.add(s);
    s.on("close", () => sockets.delete(s));
  });
  await new Promise<void>((ok) => server!.listen(SUPABASE_PORT, "127.0.0.1", ok));
}

async function stopFake(): Promise<void> {
  if (!server) return;
  for (const s of sockets) s.destroy();
  await new Promise((ok) => server!.close(ok));
  server = null;
}

async function setMode(mode: Mode): Promise<void> {
  for (const res of fake.hung.splice(0)) res.socket?.destroy();
  if (mode === "down") await stopFake();
  else await startFake();
  fake.mode = mode;
}

/** A TCP port that accepts and never answers (for the SSH host-key probe). */
let blackHole: NetServer;
let blackHolePort = 0;

// -- The app ------------------------------------------------------------------

let app: ChildProcess;
let browser: Browser;
let page: Page;
let fixture: ReturnType<typeof makeFixture>;
let logPath = "";
let shotNo = 0;
const trail: string[] = [];

function note(what: string) {
  trail.push(what);
  console.log(`[machines-chaos] ${what}`);
}

/** A fresh app on the fixture's home, connected to Playwright. */
async function launch(): Promise<void> {
  const log = openSync(logPath, "a");
  app = spawn(appBinary, [], {
    stdio: ["ignore", log, log],
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(appPort),
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
      RUST_LOG: process.env.RUST_LOG ?? "info",
    },
  });
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${appPort}/`)).ok) break;
    } catch {}
    if (Date.now() > deadline) throw new Error("machines app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
  for (let attempt = 1; ; attempt++) {
    try {
      browser = await chromium.connect(`ws://127.0.0.1:${appPort}/`);
      page = await (await browser.newContext()).newPage();
      break;
    } catch (error) {
      await browser?.close().catch(() => {});
      if (attempt >= 10) throw error;
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
  await page.setViewportSize({ width: 1400, height: 900 });
}

function appLog(): string {
  try {
    return readFileSync(logPath, "utf8");
  } catch {
    return "";
  }
}

/** The app is alive, answers Playwright and has not panicked. */
async function alive(what: string): Promise<void> {
  expect(app.exitCode, `app exited after: ${what}\ntrail: ${trail.join(" | ")}`).toBeNull();
  expect(app.signalCode, `app killed after: ${what}`).toBeNull();
  const answered = await Promise.race([
    page.ariaSnapshot().then(() => true, () => false),
    new Promise<boolean>((r) => setTimeout(() => r(false), 15_000)),
  ]);
  expect(answered, `Playwright got no answer after: ${what}\ntrail: ${trail.join(" | ")}`).toBe(
    true,
  );
  const log = appLog();
  const at = log.indexOf("panicked");
  expect(at, `panic after ${what}:\n${log.slice(Math.max(0, at - 400), at + 1500)}`).toBe(-1);
}

async function shot(what: string, title: string, expectations: string[]): Promise<void> {
  shotNo += 1;
  const name = `machines-chaos-${String(shotNo).padStart(2, "0")}-${what}`;
  await page.waitForTimeout(800);
  await capture(page, { name, title, screen: true, rest: false, expectations });
  note(`shot ${name}-${theme}.png`);
}

/** What the screen says, for the report. */
async function texts(): Promise<string> {
  const snap = await page.ariaSnapshot().catch((e) => `aria failed: ${e}`);
  return snap.length > 4000 ? `${snap.slice(0, 4000)}…` : snap;
}

const cloudRow = () => page.getByRole("listitem", { name: "Cloud workspace", exact: true });
const create = () =>
  page.getByRole("button", { name: /^(Create cloud workspace|Retry cloud workspace creation)$/ });
const creating = () => page.getByRole("button", { name: "Creating..." });

/** The Treq Cloud setup dialog, opened from its row. */
async function openCloud(): Promise<void> {
  if ((await create().count()) + (await creating().count()) > 0) return;
  await cloudRow().click();
  await page.waitForTimeout(800);
}

async function closeDialog(): Promise<void> {
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
  const close = page.getByRole("button", { name: "Close", exact: true });
  if ((await close.count()) > 0 && (await close.first().isVisible().catch(() => false))) {
    await close.first().click().catch(() => {});
    await page.waitForTimeout(500);
  }
}

/** Waits until provisioning stops (the button is back), up to `ms`. */
async function settled(ms: number): Promise<boolean> {
  try {
    await expect(create()).toBeVisible({ timeout: ms });
    return true;
  } catch {
    return false;
  }
}

// -- Setup --------------------------------------------------------------------

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  await startFake();
  blackHole = netServer((s) => sockets.add(s));
  await new Promise<void>((ok) => blackHole.listen(0, "127.0.0.1", ok));
  blackHolePort = (blackHole.address() as { port: number }).port;

  fixture = makeFixture("treq-qa-machines-");
  logPath = join(fixture.root, "app.log");
  const ssh = join(fixture.home, ".ssh");
  mkdirSync(ssh, { recursive: true, mode: 0o700 });
  execFileSync("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", join(ssh, "id_ed25519")]);
  const { DatabaseSync } = await import("node:sqlite");
  const dir = join(fixture.home, ".local/share/com.treq");
  mkdirSync(dir, { recursive: true });
  const db = new DatabaseSync(join(dir, "treq.db"));
  db.exec("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
  const set = db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)");
  set.run("theme", theme);
  set.run("supabase_session", JSON.stringify({ accessToken: "a0", refreshToken: "r0" }));
  db.close();
  writeFileSync(join(fixture.root, "seed.txt"), String(seed0));
  console.log(`[machines-chaos] seed=${seed0} home=${fixture.home} log=${logPath}`);
  await launch();
});

test.afterAll(async () => {
  console.log(`[machines-chaos] trail:\n${trail.join("\n")}`);
  console.log(`[machines-chaos] fake supabase calls:\n${fake.calls.join("\n")}`);
  await browser?.close().catch(() => {});
  app?.kill("SIGTERM");
  await stopFake();
  blackHole?.close();
});

// -- Scenarios ----------------------------------------------------------------

test("machines-chaos: overview signed in", async () => {
  await expect(cloudRow()).toBeVisible({ timeout: 60_000 });
  await alive("launch");
  await shot("overview", "Overview signed in to the fake Supabase", [
    "Machines lists This computer, SSH Hosts with its +, and Treq Cloud with 'Cloud workspace / Not created yet'",
  ]);
});

test("machines-chaos: own-VM form names", async () => {
  const names: [string, string][] = [
    ["empty", ""],
    ["whitespace", "   \t  "],
    ["unicode", "名前 🚀 ‮evil‬ Zalgo z̷̢̛a̴l̶g̸o"],
    ["10k", "x".repeat(10_000)],
  ];
  await page.getByRole("button", { name: "Add SSH host" }).click();
  await page.waitForTimeout(1000);
  const fields = page.getByRole("textbox");
  for (const [what, value] of names) {
    note(`form: display name and hostname = ${what}`);
    await fields.nth(0).fill(value);
    await fields.nth(1).fill(value);
    await fields.nth(2).fill("22");
    await fields.nth(3).fill(value);
    await alive(`form ${what}`);
    const disabled = await page
      .getByRole("button", { name: "Continue" })
      .isDisabled()
      .catch(() => "missing");
    note(`form ${what}: Continue disabled=${disabled}`);
    await shot(`form-${what}`, `Own-VM form with ${what} name/hostname/user`, [
      what === "empty" || what === "whitespace"
        ? "Continue is disabled: hostname and username are blank"
        : "Fields scroll their text; the dialog keeps its width, nothing overflows",
    ]);
  }
  // A 10k hostname, submitted.
  await page.getByRole("button", { name: "Continue" }).click().catch((e) => note(`continue: ${e}`));
  await page.waitForTimeout(4000);
  await alive("continue with 10k hostname");
  note(`after 10k continue: ${(await texts()).slice(0, 600)}`);
  await shot("form-10k-continue", "Continue with a 10k-char hostname", [
    "A readable error (bad hostname), not a frozen or overflowing dialog",
  ]);
  // A host that accepts and never answers.
  await fields.nth(0).fill("Black hole");
  await fields.nth(1).fill("127.0.0.1");
  await fields.nth(2).fill(String(blackHolePort));
  await fields.nth(3).fill("qa");
  await page.getByRole("button", { name: "Continue" }).click().catch((e) => note(`continue: ${e}`));
  await page.waitForTimeout(3000);
  await alive("probe black hole");
  // Double-submit while probing.
  await page.getByRole("button", { name: /Continue|Checking|Resolving/ }).first().dblclick().catch(() => {});
  await shot("form-blackhole-probing", "Probing a host that never answers", [
    "The form shows it is checking the host, Continue is disabled, Cancel still works",
  ]);
  await page.waitForTimeout(30_000);
  await alive("probe black hole +30s");
  note(`blackhole +33s: ${(await texts()).slice(0, 800)}`);
  await shot("form-blackhole-33s", "33 s into probing a host that never answers", [
    "Either a timeout error or still checking with a way out",
  ]);
  await page.getByRole("button", { name: "Cancel" }).first().click().catch((e) => note(`cancel: ${e}`));
  await page.waitForTimeout(1000);
  await closeDialog();
  await alive("cancel probe");
  await shot("form-cancelled", "After cancelling the probe", [
    "Back on the Overview; no host added; no stray dialog",
  ]);
});

test("machines-chaos: provision ok, double-click, close and navigate mid-provision", async () => {
  await setMode("ok");
  fake.instance = null;
  await openCloud();
  await expect(create()).toBeVisible({ timeout: 20_000 });
  await shot("cloud-dialog", "Cloud setup dialog before creating", [
    "Create cloud workspace button, enabled",
  ]);
  const before = fake.calls.filter((c) => c.includes(" ensure")).length;
  note("triple-click Create cloud workspace");
  const box = await create().boundingBox();
  if (!box) throw new Error("no Create button");
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.click(cx, cy, { clickCount: 2 });
  await page.mouse.click(cx, cy);
  await page.waitForTimeout(4000);
  await alive("triple-click create");
  const ensures = fake.calls.filter((c) => c.includes(" ensure")).length - before;
  const registers = fake.calls.filter((c) => c.includes("register_client_key")).length;
  note(`ensure calls from one triple-click: ${ensures}; register_client_key so far: ${registers}`);
  await shot("cloud-creating", "Provisioning in progress after a triple-click", [
    "Creating... spinner with a 'Waiting for the cloud workspace (provisioning)...' stage",
    "Only one provision request reached the server",
  ]);
  note("close dialog mid-provision");
  await closeDialog();
  await alive("close mid-provision");
  await shot("cloud-closed-mid", "Dialog closed while provisioning", [
    "Overview; the cloud row reflects provisioning",
  ]);
  note("navigate to Settings > Account and back mid-provision");
  await page.getByRole("button", { name: "Settings", exact: true }).click().catch((e) => note(`settings: ${e}`));
  await page.waitForTimeout(1000);
  await page.getByRole("tab", { name: "Account" }).click().catch((e) => note(`account tab: ${e}`));
  await page.waitForTimeout(1500);
  await alive("settings account mid-provision");
  await shot("settings-account-mid", "Settings > Account while provisioning", [
    "The cloud card shows the same Creating... state as the dialog",
  ]);
  await page.getByRole("button", { name: "Close", exact: true }).click().catch((e) => note(`close settings: ${e}`));
  await page.waitForTimeout(1000);
  await openCloud();
  await alive("reopen mid-provision");
  note(`reopened: ${(await texts()).slice(0, 800)}`);
  await shot("cloud-reopened-mid", "Cloud dialog reopened while provisioning", [
    "Still Creating... with its stage; Create is not offered again",
  ]);
  note("backend dies (500) mid-provision");
  await setMode("e500");
  const done = await settled(20_000);
  await alive("500 mid-provision");
  note(`settled after 500 mid-provision: ${done}; ${(await texts()).slice(0, 800)}`);
  await shot("cloud-500-mid", "The status poll gets a 500 mid-provision", [
    "Provisioning stops with a readable error and a retry",
  ]);
});

const failures: [Mode, string][] = [
  ["e500", "500 from the Edge Function"],
  ["e401", "401 Invalid JWT"],
  ["malformed", "truncated JSON"],
  ["reset", "connection reset"],
  ["down", "nothing listening"],
];

for (const [mode, what] of failures) {
  test(`machines-chaos: provision against ${mode}`, async () => {
    await setMode("ok");
    fake.instance = null;
    await closeDialog();
    await openCloud();
    await expect(create()).toBeVisible({ timeout: 30_000 });
    await setMode(mode);
    note(`create against ${mode}`);
    const t0 = Date.now();
    await create().click();
    const done = await settled(30_000);
    note(`${mode}: settled=${done} after ${Date.now() - t0}ms; ${(await texts()).slice(0, 900)}`);
    await alive(`provision ${mode}`);
    await shot(`provision-${mode}`, `Create cloud workspace against ${what}`, [
      "The button is back (Create or Retry) and a short readable error says what failed",
      "No raw JSON, Rust type names or serde text in the message",
    ]);
  });
}

test("machines-chaos: provision against a black hole", async () => {
  test.setTimeout(HANG_WAIT + 120_000);
  await setMode("ok");
  fake.instance = null;
  await closeDialog();
  await openCloud();
  await expect(create()).toBeVisible({ timeout: 30_000 });
  await setMode("hang");
  note("create against black hole");
  const t0 = Date.now();
  await create().click();
  await page.waitForTimeout(5000);
  await alive("hang +5s");
  await shot("hang-5s", "Create against a server that never answers, 5 s in", [
    "Creating... with a stage; the dialog can still close",
  ]);
  note("navigate away and back during the hang");
  await closeDialog();
  await page.setViewportSize({ width: 400, height: 300 });
  await page.waitForTimeout(800);
  await alive("tiny during hang");
  await shot("hang-tiny", "400x300 during the hang", ["Overview reflows; nothing overlaps unreadably"]);
  await page.setViewportSize({ width: 1400, height: 900 });
  await openCloud();
  const done = await settled(HANG_WAIT);
  note(`hang: settled=${done} after ${Date.now() - t0}ms; ${(await texts()).slice(0, 900)}`);
  await alive("hang end");
  await shot("hang-end", `Black hole after ${Math.round((Date.now() - t0) / 1000)} s`, [
    "A timeout error and a retry (the client times out at 160 s)",
  ]);
  await setMode("ok");
});

test("machines-chaos: sizes and zoom", async () => {
  await openCloud();
  for (const [w, h] of [
    [400, 300],
    [1900, 1180],
  ]) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(1000);
    await alive(`resize ${w}x${h}`);
    await shot(`size-${w}x${h}`, `Cloud dialog at ${w}x${h}`, [
      "The dialog fits or scrolls; its buttons stay reachable",
    ]);
  }
  await page.setViewportSize({ width: 1400, height: 900 });
  for (let i = 0; i < 12; i++) await page.keyboard.press("Control+=");
  await page.waitForTimeout(1000);
  await alive("zoom max");
  await shot("zoom-max", "Cloud dialog at maximum zoom", ["Text large; dialog scrolls, not clipped"]);
  await page.keyboard.press("Control+0");
  for (let i = 0; i < 12; i++) await page.keyboard.press("Control+-");
  await page.waitForTimeout(1000);
  await alive("zoom min");
  await shot("zoom-min", "Cloud dialog at minimum zoom", ["Small but laid out"]);
  await page.keyboard.press("Control+0");
  await closeDialog();
});

// -- The monkey ---------------------------------------------------------------

function random(): number {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = <T>(items: T[]): T => items[Math.floor(random() * items.length)];

/** Buttons that leave the machines screens or block on a native dialog. */
const AVOID = /Open repository|Sign in|Upgrade|Settings|Command palette|Delete|Remove host|Reset terminals|Use tmux|Learn|Docs|Open in/i;

async function monkeyStep(i: number): Promise<string> {
  const kind = pick([
    "button", "button", "button", "row", "key", "key", "type", "mode", "size", "zoom",
  ]);
  switch (kind) {
    case "button": {
      const buttons = page.getByRole("button");
      const n = await buttons.count();
      const names: string[] = [];
      for (let k = 0; k < n && k < 60; k++) {
        const name = (await buttons.nth(k).getAttribute("aria-label").catch(() => null)) ??
          (await buttons.nth(k).textContent().catch(() => "")) ?? "";
        names.push(name);
      }
      const ok = names.map((nm, k) => [nm, k] as const).filter(([nm]) => nm && !AVOID.test(nm));
      if (!ok.length) return "button (none)";
      const [nm, k] = pick(ok);
      await buttons.nth(k).click({ timeout: 3000 }).catch(() => {});
      return `button "${nm.slice(0, 40)}"`;
    }
    case "row": {
      const rows = page.getByRole("listitem");
      const n = await rows.count();
      if (!n) return "row (none)";
      const k = Math.floor(random() * n);
      await rows.nth(k).click({ timeout: 3000 }).catch(() => {});
      return `row #${k}`;
    }
    case "key": {
      const key = pick(["Escape", "Escape", "Tab", "Shift+Tab", "Enter", "Space", "Control+k", "Control+n"]);
      await page.keyboard.press(key);
      return `key ${key}`;
    }
    case "type": {
      const text = pick(["", " ", "qa-host", "🚀名前", "x".repeat(500), "127.0.0.1", "-1", "99999"]);
      await page.keyboard.type(text);
      return `type ${JSON.stringify(text.slice(0, 20))}(${text.length})`;
    }
    case "mode": {
      const mode = pick<Mode>(["ok", "ok", "e500", "e401", "malformed", "reset", "down"]);
      await setMode(mode);
      if (random() < 0.3) fake.instance = null;
      return `backend ${mode}`;
    }
    case "size": {
      const [w, h] = pick([[400, 300], [800, 600], [1400, 900], [1900, 1180]]);
      await page.setViewportSize({ width: w, height: h });
      return `size ${w}x${h}`;
    }
    default: {
      const key = pick(["Control+=", "Control+-", "Control+0"]);
      await page.keyboard.press(key);
      return `zoom ${key}`;
    }
  }
  void i;
}

test("machines-chaos: seeded monkey", async () => {
  test.setTimeout(1_200_000);
  note(`monkey seed=${seed0} steps=${STEPS}`);
  for (let i = 0; i < STEPS; i++) {
    const did = await monkeyStep(i);
    note(`monkey ${i}: ${did}`);
    await page.waitForTimeout(250);
    await alive(`monkey ${i}: ${did}`);
    if (i % 20 === 19) await shot(`monkey-${i + 1}`, `Monkey after ${i + 1} steps`, ["App drawn, not blank"]);
  }
  await setMode("ok");
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.keyboard.press("Control+0");
  await page.waitForTimeout(1000);
  await alive("monkey end");
});
