// `npm run qa`: runs each QA spec file against its own app on a fresh seed.
// Specs change the demo repository (merge, archive, resolve a conflict),
// so one app shared by the whole run lets an early spec break a later
// one. Arguments pass through to Playwright (`npm run qa -- --grep
// commits`); only the files they select start an app. With QA_REUSE=1 it
// is a plain run against the app already up on the port.
import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
const playwright = (extra: string[], stdio: "pipe" | "inherit" = "inherit") =>
  spawnSync("npx", ["playwright", "test", "-c", "qa/qa.config.ts", ...extra], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    stdio: stdio === "pipe" ? ["ignore", "pipe", "pipe"] : "inherit",
  });

/**
 * Under Xvfb on a software renderer, wgpu now and then panics on the
 * window's first frame (`create_window_surface: BadNativeWindow`, then
 * `Surface::get_current_texture_view`), before any test reaches the app.
 * Only a run that died that way is started again, up to twice, and it says so.
 */
const STARTUP_CRASH = /Surface::get_current_texture_view: Validation Error/;

function runFile(file: string): boolean {
  for (let attempt = 1; ; attempt++) {
    const run = playwright([...options, `qa/${file}`], "pipe");
    process.stdout.write(run.stdout);
    process.stderr.write(run.stderr);
    if (run.status === 0) return true;
    if (attempt > 2 || !STARTUP_CRASH.test(run.stdout + run.stderr)) return false;
    console.log(`\n=== qa/${file}: the app crashed on its first frame (wgpu); starting it again`);
  }
}

if (process.env.QA_REUSE) process.exit(playwright(args).status ?? 1);

// Spec files named on the command line or matched by --grep.
const listed = playwright([...args, "--list"], "pipe");
if (listed.status !== 0) process.exit(listed.status ?? 1);
const files = [
  ...new Set(
    listed.stdout
      .split("\n")
      .map((line) => /^\s+(\S+\.spec\.ts):\d+:\d+ /.exec(line)?.[1])
      .filter((file): file is string => !!file),
  ),
];
const options = args.filter((arg) => !/\.spec\.ts(:\d+)*$/.test(arg));

const failed: string[] = [];
for (const file of files) {
  console.log(`\n=== qa/${file}`);
  if (!runFile(file)) failed.push(file);
}
console.log(`\n${files.length - failed.length}/${files.length} spec files passed`);
for (const file of failed) console.log(`  failed: qa/${file}`);
process.exit(failed.length ? 1 : 0);
