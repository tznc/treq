// Starts the desktop app for the suite: a fresh home and app data dir, a
// git repository to open, and a fake `claude` agent on PATH, then the
// `treq` binary built with `--features e2e`. Playwright's `webServer` runs
// it and waits for gpui-playwright's port; the tests read the fixture from
// `fixture.json` (see `fixture.ts`).
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { gitIn, makeFixture, startApp, writeFakeAgent } from "./harness.ts";

const fixture = makeFixture("treq-e2e-");
const { repo, home } = fixture;
const git = (...args: string[]) => gitIn(repo, home, ...args);
git("init", "-b", "main");
writeFileSync(join(repo, "README.md"), "# e2e fixture\n");
writeFileSync(join(repo, "app.txt"), "first line\n");
git("add", ".");
git("commit", "-m", "Initial commit");

writeFakeAgent(fixture);
startApp(fixture);
