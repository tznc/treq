// Switching workspaces and coming back to the Code tab, recorded as a
// video: the frames between a click and the loaded view show whether the
// header, tab row or tab body flash empty while the next workspace loads.
// Writes `15-workspace-switch-<theme>.mp4` (and a GIF of it) into `QA_OUT`.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { ALERTING, CACHE, MARKETING } from "../landing/seed.ts";
import { outDir, theme } from "./capture.ts";
import { openWorkspace, row, settle } from "./helpers.ts";

const scale = Number(process.env.GDK_SCALE ?? "2");

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

async function select(page: Page, branch: string) {
  await row(page, branch).click();
  await expect(page.getByRole("button", { name: branch, exact: true, selected: true })).toBeVisible();
  await settle(page, 1500);
}

async function tab(page: Page, name: string) {
  await page.getByRole("tab", { name }).click();
  await expect(page.getByRole("tab", { name, selected: true })).toBeVisible();
  await settle(page, 1500);
}

test("workspace-switch: switching workspaces and back to the Code tab", async ({ page }) => {
  await openWorkspace(page, MARKETING);
  await tab(page, "Code");
  await settle(page, 2000);
  mkdirSync(outDir, { recursive: true });
  const video = join(outDir, `15-workspace-switch-${theme}.mp4`);
  const stop = record(video);
  await settle(page, 800);
  await select(page, CACHE);
  await select(page, ALERTING);
  await select(page, MARKETING);
  await tab(page, "Commits");
  await tab(page, "Code");
  await tab(page, "Changes");
  await tab(page, "Code");
  await stop();
  execFileSync("ffmpeg", [
    "-y", "-loglevel", "error", "-i", video,
    "-vf", "fps=15,scale=900:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
    video.replace(/\.mp4$/, ".gif"),
  ]);
});
