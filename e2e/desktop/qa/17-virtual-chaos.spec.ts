// Chaos on the virtual lists in the real app: a seeded run of random wheel
// scrolls, clicks, hovers, keys and zoom steps over the Changes diff of
// five 500-line files, then over the same files' commit in the Commits
// tab. After every step the app must still answer, and every few steps
// the list area must not be blank (a frame of one flat colour). Records
// the window to `17-virtual-chaos-<theme>.mp4` (and a GIF) in `QA_OUT`.
// `SEED` and `CHAOS_STEPS` vary the run.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { CACHE, workspaceDir } from "../landing/seed.ts";
import { outDir, theme } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const scale = Number(process.env.GDK_SCALE ?? "2");
const FILES = 5;
const LINES = 500;
const STEPS = Number(process.env.CHAOS_STEPS ?? 60);
let seed = Number(process.env.SEED ?? 7);
/** Zoom steps taken, to undo at the end. */
let zoom = 0;

/** mulberry32: the run repeats for a seed. */
function random(): number {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}

const pick = <T>(items: T[]): T => items[Math.floor(random() * items.length)];
const between = (lo: number, hi: number) => lo + Math.floor(random() * (hi - lo));

/** The checkout of the running QA app's cache workspace. */
function cacheDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const { repo } = JSON.parse(readFileSync(join(roots[0], "qa-extra.json"), "utf8"));
  return workspaceDir(repo, CACHE);
}

function source(f: number): string {
  return Array.from(
    { length: LINES },
    (_, i) =>
      i % 25 === 0
        ? `// Section ${i / 25} of module ${f}: lorem ipsum dolor sit amet`
        : `export const value${i} = compute${f}("item ${i}", ${i * 3}).map((v) => v + ${i}); // ${i}`,
  ).join("\n");
}

function record(path: string): () => Promise<void> {
  const size = `${1400 * scale}x${900 * scale}`;
  const ffmpeg: ChildProcess = spawn(
    "ffmpeg",
    ["-y", "-loglevel", "error", "-f", "x11grab", "-framerate", "15", "-video_size", size,
      "-i", `${process.env.DISPLAY}+0,0`, "-vf", "scale=1400:-2", "-pix_fmt", "yuv420p", path],
    { stdio: ["pipe", "inherit", "inherit"] },
  );
  return () =>
    new Promise((done) => {
      ffmpeg.on("exit", () => done());
      ffmpeg.stdin?.write("q");
    });
}

/** How much the area (logical pixels) varies: about 0 when it is blank. */
function spread(name: string, x: number, y: number, w: number, h: number): number {
  mkdirSync(outDir, { recursive: true });
  const png = join(outDir, `17-virtual-chaos-${name}-${theme}.png`);
  execFileSync("import", ["-window", "root", "-crop",
    `${w * scale}x${h * scale}+${x * scale}+${y * scale}`, png]);
  return Number(
    execFileSync("convert", [png, "-format", "%[fx:standard_deviation]", "info:"], {
      encoding: "utf8",
    }),
  );
}

type Area = { x: number; y: number; w: number; h: number };

/** One random step inside `area`; returns what it did. */
async function step(page: Page, area: Area): Promise<string> {
  const at = () => ({ x: between(area.x, area.x + area.w), y: between(area.y, area.y + area.h) });
  const kind = pick(["wheel", "wheel", "wheel", "click", "hover", "hover", "key", "key", "zoom"]);
  switch (kind) {
    case "wheel": {
      const dy = pick([-1, 1]) * pick([120, 400, 1200, 5000]);
      const p = at();
      await page.mouse.move(p.x, p.y);
      await page.mouse.wheel(0, dy);
      return `wheel ${dy} at ${p.x},${p.y}`;
    }
    case "click": {
      const p = at();
      await page.mouse.click(p.x, p.y);
      return `click ${p.x},${p.y}`;
    }
    case "hover": {
      const p = at();
      await page.mouse.move(p.x, p.y, { steps: 4 });
      return `hover ${p.x},${p.y}`;
    }
    case "key": {
      const key = pick([
        "ArrowDown", "ArrowUp", "Shift+ArrowDown", "PageDown", "PageUp", "Home", "End",
        "Escape", "Escape",
      ]);
      await page.keyboard.press(key);
      return `key ${key}`;
    }
    default: {
      const by = pick([1, -1]);
      zoom += by;
      await page.keyboard.press(by > 0 ? "Control+=" : "Control+-");
      return `zoom ${by > 0 ? "in" : "out"}`;
    }
  }
}

async function chaos(page: Page, name: string, area: Area, alive: () => Promise<void>) {
  const log: string[] = [];
  for (let i = 0; i < STEPS; i++) {
    log.push(await step(page, area));
    // The app answers once the frames the step asked for are drawn.
    await alive();
    if (i % 10 === 9) {
      await settle(page, 400);
      const s = spread(`${name}-${i + 1}`, area.x, area.y, area.w, area.h);
      expect(s, `blank after ${log.join("; ")}`).toBeGreaterThan(0.01);
    }
  }
  // Back to 100%: zoom steps are 5% each way.
  for (; zoom > 0; zoom--) await page.keyboard.press("Control+-");
  for (; zoom < 0; zoom++) await page.keyboard.press("Control+=");
  console.log(`virtual-chaos ${theme} ${name}: ${log.join("; ")}`);
}

test("virtual-chaos: random input on large virtual lists", async ({ page }) => {
  test.setTimeout(1_800_000);
  const dir = cacheDir();
  mkdirSync(join(dir, "src/perf"), { recursive: true });
  for (let f = 0; f < FILES; f++) writeFileSync(join(dir, `src/perf/module${f}.ts`), `${source(f)}\n`);

  await openWorkspace(page, CACHE);
  await openTab(page, "Changes");
  await page.getByRole("button", { name: "View changes" }).first().click();
  await settle(page, 1000);
  let opened = 0;
  for (let i = 0; i < 200 && opened < FILES; i++) {
    const more = page.getByRole("button", { name: "View changes" });
    const box = (await more.count()) > 0 ? await more.first().boundingBox() : null;
    if (box && box.y > 80 && box.y + box.height < 880) {
      await more.first().click();
      opened += 1;
      await settle(page, 300);
    } else {
      await page.mouse.wheel(0, 800);
      await settle(page, 100);
    }
  }
  expect(opened).toBe(FILES);
  await page.mouse.wheel(0, -100_000);
  await settle(page, 1000);

  mkdirSync(outDir, { recursive: true });
  const video = join(outDir, `17-virtual-chaos-${theme}.mp4`);
  const stop = record(video);
  try {
    // The diff, left of its cards' header buttons.
    await chaos(page, "changes", { x: 560, y: 140, w: 560, h: 720 }, async () => {
      await expect(page.getByRole("tab", { name: "Changes" })).toBeVisible();
    });

    // Commit the files, then open the commit.
    await page.getByRole("textbox", { name: "Message" }).fill("chaos: five large modules");
    await page.getByRole("button", { name: /^Commit( \d+ files?)?$/ }).click();
    await settle(page, 3000);
    await openTab(page, "Commits");
    await settle(page, 1500);
    const undo = page.getByRole("button", { name: "Undo commit" });
    for (let y = 120; y < 400 && (await undo.count()) === 0; y += 20) {
      await page.mouse.click(700, y);
      await settle(page, 600);
    }
    await expect(undo).toBeVisible();
    await settle(page, 1500);
    await chaos(page, "commits", { x: 360, y: 140, w: 760, h: 720 }, async () => {
      await expect(page.getByRole("tab", { name: "Commits" })).toBeVisible();
    });
    await settle(page, 600);
  } finally {
    await stop();
  }
  execFileSync("ffmpeg", [
    "-y", "-loglevel", "error", "-i", video,
    "-vf", "fps=8,scale=900:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
    video.replace(/\.mp4$/, ".gif"),
  ]);
});
