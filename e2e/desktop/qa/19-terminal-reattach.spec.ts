// Terminals outlive the window they opened in while another window is
// open: a shell keeps running when its workspace view gives way to
// Settings, and when its window closes, the next window on the repository
// shows it again, scrollback and all. Recorded as
// `19-terminal-reattach-<theme>.mp4` (and a GIF of it) in `QA_OUT`.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { dialogPage } from "../fixture.ts";
import { capture, outDir, theme } from "./capture.ts";
import { mod, openTab, openWorkspace, overlay, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";
const scale = Number(process.env.GDK_SCALE ?? "2");

/** Records the repo window (at the screen's origin) until `stop`. */
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

test.describe.configure({ mode: "serial" });

/** The running QA app's demo repository. */
function repoDir(): string {
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
  return JSON.parse(readFileSync(join(roots[0], "qa-extra.json"), "utf8")).repo;
}

/** The repo windows' X ids, oldest first. */
function repoWindows(): string[] {
  return execFileSync("xdotool", ["search", "--onlyvisible", "--name", "^Treq - "])
    .toString()
    .trim()
    .split("\n")
    .filter(Boolean);
}

/**
 * Asks window `id` to close as its title bar's close button would: the
 * WM_DELETE_WINDOW message a window manager sends (Xvfb runs none).
 */
function closeWindow(id: string): void {
  const script = `
import ctypes, sys
x = ctypes.cdll.LoadLibrary("libX11.so.6")
x.XOpenDisplay.restype = ctypes.c_void_p
x.XInternAtom.restype = ctypes.c_ulong
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.c_void_p]
x.XFlush.argtypes = [ctypes.c_void_p]
d = x.XOpenDisplay(None)
class Message(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
                ("format", ctypes.c_int), ("l", ctypes.c_long * 5)]
class Event(ctypes.Union):
    _fields_ = [("xclient", Message), ("pad", ctypes.c_long * 24)]
e = Event()
e.xclient.type = 33
e.xclient.window = int(sys.argv[1])
e.xclient.message_type = x.XInternAtom(d, b"WM_PROTOCOLS", 0)
e.xclient.format = 32
e.xclient.l[0] = x.XInternAtom(d, b"WM_DELETE_WINDOW", 0)
x.XSendEvent(d, int(sys.argv[1]), 0, 0, ctypes.byref(e))
x.XFlush(d)
`;
  execFileSync("python3", ["-c", script, id]);
}

/** Picks `folder` in the GTK Select Folder dialog. */
async function pickFolder(folder: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  let dialog = "";
  while (!dialog) {
    try {
      dialog = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "Select Folder"])
        .toString()
        .trim()
        .split("\n")[0];
    } catch {
      if (Date.now() > deadline) throw new Error("no folder picker");
      await new Promise((r) => setTimeout(r, 300));
    }
  }
  execFileSync("xdotool", ["windowfocus", "--sync", dialog]);
  const geo = Object.fromEntries(
    execFileSync("xdotool", ["getwindowgeometry", "--shell", dialog], { encoding: "utf8" })
      .split("\n")
      .filter(Boolean)
      .map((l) => l.split("=")),
  );
  // It opens on Recent, where a typed path only searches: Home first.
  execFileSync("xdotool", ["key", "--window", dialog, "alt+Home"]);
  await new Promise((r) => setTimeout(r, 800));
  execFileSync("xdotool", ["key", "--window", dialog, "ctrl+l"]);
  await new Promise((r) => setTimeout(r, 500));
  // The path, then Return to take the completion and Return to go in.
  execFileSync("xdotool", ["type", "--window", dialog, "--delay", "20", `${folder}/`]);
  await new Promise((r) => setTimeout(r, 800));
  for (let i = 0; i < 2; i++) {
    execFileSync("xdotool", ["key", "--window", dialog, "Return"]);
    await new Promise((r) => setTimeout(r, 800));
  }
  // Open, at the dialog's bottom right (GDK_SCALE=2).
  const x = Number(geo.X) + Number(geo.WIDTH) - 98;
  const y = Number(geo.Y) + Number(geo.HEIGHT) - 48;
  execFileSync("xdotool", ["mousemove", String(x), String(y), "click", "1"]);
  await new Promise((r) => setTimeout(r, 1500));
}

test("terminal-reattach: a closed window's terminals open again", async ({ page, context }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  mkdirSync(outDir, { recursive: true });
  const video = join(outDir, `19-terminal-reattach-${theme}.mp4`);
  const stop = record(video);
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type("PS1='event-bus $ '; clear\n");
  await settle(page, 600);
  await page.keyboard.type("echo opened in the first window; for i in 1 2 3; do echo line $i; done\n");
  await settle(page, 1200);
  await capture(page, {
    name: "19-terminal-reattach-01-first-window",
    title: "A shell in the first window",
    expectations: [
      "The pane shows one Shell column with 'opened in the first window' and lines 1-3",
    ],
  });

  // Away to Settings and back: the same shell, still running.
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("button", { name: "Close", exact: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 1500);
  await capture(page, {
    name: "19-terminal-reattach-02-settings",
    title: "Settings in place of the workspace",
    expectations: ["Settings fills the window; no terminal pane shows"],
  });
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await page.mouse.click(1000, 780);
  await page.keyboard.type("echo back from settings\n");
  await settle(page, 1200);
  await capture(page, {
    name: "19-terminal-reattach-03-back-from-settings",
    title: "Back in the workspace after Settings",
    expectations: [
      "The same Shell column shows the earlier lines, then 'back from settings'",
    ],
  });

  // A second window on the repository, then the first one closes.
  const first = repoWindows()[0];
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("new window");
  await settle(page, 600);
  await palette.getByRole("option", { name: "Open Repository in New Window" }).click();
  await pickFolder(repoDir());
  await expect.poll(() => repoWindows().length, { timeout: 30_000 }).toBe(2);
  const second = await dialogPage(context);
  await second.setViewportSize({ width: 1400, height: 900 });
  await expect(second.getByRole("button", { name: "Command palette" })).toBeVisible({
    timeout: 30_000,
  });
  await settle(second, 2000);
  await capture(second, {
    name: "19-terminal-reattach-04-second-window",
    title: "A second window on the repository, the first still open",
    expectations: ["The second window shows no terminals: the first window still has the shell"],
  });
  closeWindow(first);
  await expect.poll(() => repoWindows().length, { timeout: 15_000 }).toBe(1);
  await settle(second, 1500);

  // Back into the workspace in the second window: the shell is there.
  await row(second, BRANCH).click();
  await settle(second, 1500);
  await second.getByRole("tab", { name: "Commits" }).click();
  await settle(second, 1500);
  await capture(second, {
    name: "19-terminal-reattach-05-reattached",
    title: "The first window's shell in the second window",
    expectations: [
      "The pane opened on its own with the Shell column",
      "Its scrollback shows 'opened in the first window', lines 1-3 and 'back from settings'",
    ],
  });
  await second.mouse.click(1000, 780);
  await second.keyboard.type("echo still the same shell: $$\n");
  await settle(second, 1500);
  await capture(second, {
    name: "19-terminal-reattach-06-typing",
    title: "Typing into the reattached shell",
    expectations: ["The shell answers below its earlier output: no new shell was started"],
  });
  await stop();
  execFileSync("ffmpeg", [
    "-y", "-loglevel", "error", "-i", video,
    "-vf", "fps=8,scale=1000:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
    video.replace(/\.mp4$/, ".gif"),
  ]);
});
