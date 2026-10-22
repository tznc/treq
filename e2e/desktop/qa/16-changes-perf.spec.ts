// The Changes tab on a large working copy: five new files of 500 lines each,
// every diff expanded. Records the window while hovering down the diff,
// selecting lines from the keyboard, scrolling and jumping to a file from
// the list, and prints how long each burst of input took to be handled:
// the app answers an input only once the frame before it is done, so a
// slow frame shows up as a slow burst. Writes `16-changes-perf-<theme>.mp4`
// (and a GIF) into `QA_OUT`, and removes the files when done.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { CACHE, workspaceDir } from "../landing/seed.ts";
import { outDir, theme } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const scale = Number(process.env.GDK_SCALE ?? "2");
const FILES = 5;
/** `PERF_LINES` runs the same steps on a small diff, for comparison. */
const LINES = Number(process.env.PERF_LINES ?? 500);

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

/** Records the repo window (at the screen's origin) until `stop`. */
function record(path: string): () => Promise<void> {
  const size = `${1400 * scale}x${900 * scale}`;
  const ffmpeg: ChildProcess = spawn(
    "ffmpeg",
    ["-y", "-loglevel", "error", "-f", "x11grab", "-framerate", "30", "-video_size", size,
      "-i", `${process.env.DISPLAY}+0,0`, "-vf", "scale=1400:-2", "-pix_fmt", "yuv420p", path],
    { stdio: ["pipe", "inherit", "inherit"] },
  );
  return () =>
    new Promise((done) => {
      ffmpeg.on("exit", () => done());
      ffmpeg.stdin?.write("q");
    });
}

async function timed(name: string, act: () => Promise<void>, page: Page): Promise<number> {
  const start = performance.now();
  await act();
  // A round trip after the burst: answered once its frames are drawn.
  await page.getByRole("tab", { name: "Changes", selected: true }).isVisible();
  const ms = performance.now() - start;
  console.log(`changes-perf ${theme} ${name}: ${ms.toFixed(0)}ms`);
  return ms;
}

test("changes-perf: a large diff stays responsive", async ({ page }) => {
  // Before virtualization each burst took minutes in a debug build.
  test.setTimeout(1_200_000);
  const dir = cacheDir();
  const paths = Array.from({ length: FILES }, (_, f) => `src/perf/module${f}.ts`);
  mkdirSync(join(dir, "src/perf"), { recursive: true });
  paths.forEach((p, f) => writeFileSync(join(dir, p), `${source(f)}\n`));
  try {
    await openWorkspace(page, CACHE);
    await openTab(page, "Changes");
    const expanding = performance.now();
    const large = LINES > 250;
    if (large) {
      await page.getByRole("button", { name: "View changes" }).first().click();
      await settle(page, 1000);
    }
    // One "View changes" per large file. Only the rows on screen are
    // drawn, so scroll down to each next one.
    let opened = 0;
    for (let i = 0; large && i < 200 && opened < FILES; i++) {
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
    expect(opened).toBe(large ? FILES : 0);
    await page.mouse.wheel(0, -100_000);
    console.log(`changes-perf ${theme} expand all: ${(performance.now() - expanding).toFixed(0)}ms`);
    await settle(page, 1500);

    mkdirSync(outDir, { recursive: true });
    const video = join(outDir, `16-changes-perf-${LINES}-${theme}.mp4`);
    const stop = record(video);
    await settle(page, 600);
    await timed("hover 20 moves", async () => {
      for (let i = 0; i < 20; i++) await page.mouse.move(900, 200 + i * 25);
    }, page);
    // Select a line from its gutter, then walk down from the keyboard.
    await page.mouse.click(600, 300);
    await timed("arrow down x10", async () => {
      for (let i = 0; i < 10; i++) await page.keyboard.press("ArrowDown");
    }, page);
    await page.keyboard.press("Escape");
    await timed("wheel x10", async () => {
      for (let i = 0; i < 10; i++) await page.mouse.wheel(0, 300);
    }, page);
    await timed("hover after scroll 20 moves", async () => {
      for (let i = 0; i < 20; i++) await page.mouse.move(900, 700 - i * 25);
    }, page);
    await settle(page, 600);
    await stop();
    execFileSync("ffmpeg", [
      "-y", "-loglevel", "error", "-i", video,
      "-vf", "fps=15,scale=900:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
      video.replace(/\.mp4$/, ".gif"),
    ]);
  } finally {
    rmSync(join(dir, "src/perf"), { recursive: true, force: true });
  }
});
