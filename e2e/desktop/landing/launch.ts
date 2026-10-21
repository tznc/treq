// Starts the app for the landing capture: the demo event-bus repository
// (seed.ts), a fake `gh` with fixture PRs, issues and checks, and a fake
// agent on PATH. Playwright's `webServer` runs it (landing.config.ts).
import { copyFileSync, symlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { appBinary } from "../env.ts";
import { makeFixture, startApp, writeBin } from "../harness.ts";
import { seedLandingRepo } from "./seed.ts";

const here = dirname(fileURLToPath(import.meta.url));
const fixture = makeFixture("treq-landing-", "event-bus");

const fakeGh = join(fixture.root, "fake-gh.mjs");
copyFileSync(join(here, "fake-gh.mjs"), fakeGh);
writeBin(
  fixture.bin,
  "gh",
  `#!/bin/sh\nexec '${process.execPath}' '${fakeGh}' "$@"\n`,
);

// `treq send` from the workspace shell.
symlinkSync(appBinary, join(fixture.bin, "treq"));

// The agent the terminal pane shows: a short, made-up session transcript,
// then it waits like an interactive CLI.
writeBin(
  fixture.bin,
  "claude",
  `#!/bin/sh
printf '%s\\n' "$*" >> '${fixture.agentLog}'
printf '\\033[1m> Handle empty event_message in postEvent\\033[0m\\n\\n'
printf '\\033[32m●\\033[0m Read packages/web/src/lib/client.ts\\n'
printf '\\033[32m●\\033[0m Fall back to JSON.stringify(body) when event_message is empty\\n'
printf '  \\033[2m└ Updated client.ts (+6 -1)\\033[0m\\n'
printf '\\033[32m●\\033[0m Ran the client tests\\n'
printf '  \\033[2m└ 1 passed\\033[0m\\n\\n'
printf '\\033[33m●\\033[0m CI on #418: test failed, typecheck pending\\n\\n'
printf '\\033[2m>\\033[0m '
exec cat >/dev/null
`,
);

seedLandingRepo(fixture);

// The image `treq send` previews.
writeFileSync(
  join(fixture.home, "research-draft.svg"),
  `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400" viewBox="0 0 640 400">
  <rect width="640" height="400" rx="16" fill="#0f172a"/>
  <text x="40" y="64" fill="#f8fafc" font-family="sans-serif" font-size="28" font-weight="700">Events per provider</text>
  <text x="40" y="96" fill="#94a3b8" font-family="sans-serif" font-size="16">Last 7 days, after the empty-message fix</text>
  <rect x="60" y="150" width="90" height="200" rx="8" fill="#6366f1"/>
  <rect x="190" y="210" width="90" height="140" rx="8" fill="#22c55e"/>
  <rect x="320" y="180" width="90" height="170" rx="8" fill="#6366f1"/>
  <rect x="450" y="240" width="90" height="110" rx="8" fill="#22c55e"/>
  <text x="105" y="380" fill="#cbd5e1" font-family="sans-serif" font-size="14" text-anchor="middle">Discord</text>
  <text x="235" y="380" fill="#cbd5e1" font-family="sans-serif" font-size="14" text-anchor="middle">Slack</text>
  <text x="365" y="380" fill="#cbd5e1" font-family="sans-serif" font-size="14" text-anchor="middle">Discord</text>
  <text x="495" y="380" fill="#cbd5e1" font-family="sans-serif" font-size="14" text-anchor="middle">Slack</text>
</svg>
`,
);

startApp(fixture, {
  // Device pixels for the PNGs: GTK, which owns the window, scales by 2.
  GDK_SCALE: process.env.GDK_SCALE ?? "2",
  TREQ_FAKE_GH_LOG: join(fixture.root, "gh-calls.log"),
});
