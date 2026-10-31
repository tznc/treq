// Chaos around rebases in the real app: a seeded run of random navigation,
// shortcuts, uncommitted edits in random workspaces and `treq commit`s (each
// rebases the committed branch's children). After every step the app must
// still answer, and no workspace may hold another workspace's uncommitted
// file: a parent's uncommitted changes never reach its children. `SEED` and
// `CHAOS_STEPS` vary the run; the trail prints on failure.
import { execFileSync } from "node:child_process";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { appBinary } from "../env.ts";
import { appEnv } from "../harness.ts";
import { ALERTING, CACHE, MARKETING, STACK_PARENT, workspaceDir } from "../landing/seed.ts";
import { mod, openWorkspace, ready, settle } from "./helpers.ts";

const STEPS = Number(process.env.CHAOS_STEPS ?? 80);
let seed = Number(process.env.SEED ?? 11);
const BRANCHES = [STACK_PARENT, MARKETING, CACHE, ALERTING];
const TABS = ["Code", "Commits", "Changes", "Logs"];
const KEYS = ["Escape", "Tab", "Enter", "ArrowDown", "ArrowUp", `${mod}+k`, `${mod}+p`, `${mod}+j`];

/** mulberry32: the run repeats for a seed. */
function random(): number {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = <T>(items: T[]): T => items[Math.floor(random() * items.length)];
const wipFile = (branch: string) => `wip-${branch.replaceAll("/", "-")}.txt`;

test("rebase chaos: uncommitted edits never cross workspaces", async ({ page }) => {
  test.setTimeout(1_800_000);
  const f = fixture();
  const env = {
    ...appEnv({ ...f, bin: join(f.root, "bin") }),
    TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
  };
  const dir = (b: string) => workspaceDir(f.repo, b);
  const trail: string[] = [];

  async function step(): Promise<void> {
    const roll = random();
    if (roll < 0.2) {
      const b = pick(BRANCHES);
      trail.push(`open ${b}`);
      await openWorkspace(page, b);
    } else if (roll < 0.4) {
      const tab = pick(TABS);
      trail.push(`tab ${tab}`);
      const t = page.getByRole("tab", { name: tab });
      if (await t.count()) await t.first().click();
    } else if (roll < 0.55) {
      const key = pick(KEYS);
      trail.push(`key ${key}`);
      await page.keyboard.press(key);
      if (key.includes("+")) await page.keyboard.press("Escape");
    } else if (roll < 0.75) {
      const b = pick(BRANCHES);
      trail.push(`dirty ${b}`);
      writeFileSync(join(dir(b), wipFile(b)), `wip ${trail.length}\n`);
    } else if (roll < 0.85) {
      const b = pick(BRANCHES);
      trail.push(`clean ${b}`);
      rmSync(join(dir(b), wipFile(b)), { force: true });
    } else {
      const b = pick(BRANCHES);
      // Commit only the branch's own edit, so a later check can tell leaks.
      const own = join(dir(b), wipFile(b));
      if (!existsSync(own)) writeFileSync(own, "commit me\n");
      const committed = `committed-${trail.length}-${wipFile(b)}`;
      execFileSync("mv", [own, join(dir(b), committed)]);
      trail.push(`commit ${b}`);
      try {
        execFileSync(appBinary, ["commit", b, "-m", `chaos ${trail.length}`], { cwd: f.repo, env, stdio: "pipe" });
      } catch (e) {
        trail.push(`  commit failed: ${String((e as { stderr?: Buffer }).stderr ?? e).trim()}`);
      }
    }
    await settle(page, 250);
  }

  async function check(): Promise<void> {
    await expect(page.getByRole("button", { name: MARKETING }).first(), trail.join("; ")).toBeVisible({ timeout: 15_000 });
    for (const b of BRANCHES) {
      for (const other of BRANCHES.filter((o) => o !== b)) {
        expect(existsSync(join(dir(b), wipFile(other))), `${wipFile(other)} leaked into ${b}: ${trail.join("; ")}`).toBe(false);
      }
    }
  }

  await ready(page);
  for (let i = 0; i < STEPS; i++) {
    await step();
    await check();
  }
  for (const b of BRANCHES) rmSync(join(dir(b), wipFile(b)), { force: true });
  console.log(`rebase-chaos seed ${process.env.SEED ?? 11}: ${trail.join("; ")}`);
});
