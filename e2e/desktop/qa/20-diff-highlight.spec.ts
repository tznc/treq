// Diff syntax highlighting across hunk lines: an Elixir `@doc` heredoc
// colors every line it spans, the closing `"""` included, and code after
// it is code again. The file row is not observable, so the step clicks a
// fixed point (the only uncommitted file, as in 14-diff-context).
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { appBinary } from "../env.ts";
import { fixture, git } from "../fixture.ts";
import { appEnv } from "../harness.ts";
import { workspaceDir, writeInWorkspace } from "../landing/seed.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/alerting-logs";
const FILE = { x: 330, y: 283 };

test("diff highlight: a heredoc spans its lines", async ({ page }) => {
  const dir = join(workspaceDir(fixture().repo, BRANCH), "lib");
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, "replica_check.ex"),
    `defmodule ReplicaCheck do
  @doc """
  Compares the reconciled tables of a replica against the primary.
  """
  @spec compare(Ecto.Repo.t() | pid()) :: check_result()
  def compare(replica_repo) do
    cutoff = NaiveDateTime.add(NaiveDateTime.utc_now(), -@lag_seconds)
  end
end
`,
  );
  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await page.mouse.click(FILE.x, FILE.y);
  await settle(page, 1500);
  await capture(page, {
    name: "20-diff-highlight-01-heredoc",
    title: "replica_check.ex diff: an @doc heredoc",
    expectations: [
      "The @doc heredoc's body line is doc colored, not Compares as a module",
      "The closing \"\"\" has the same color as the body and opening \"\"\"",
      "@spec and def after the heredoc are colored as code again",
    ],
  });
});

// A hunk whose first context line is a docstring's closing `"""`: read on
// its own it opens a heredoc, which drew the rest of the hunk as a string.
const LOCK_BRANCH = "fix/backends-lock";
const LOCK_FILE = "lib/backends.ex";
const LOCK_COMMIT = "fix: lock backends before deleting them";
const backends = (query: string) => `defmodule Backends do
  @doc """
  Deletes a user's backends.

  Returns the count and the deleted backends.
  """
  @spec delete_user_backends(User.t()) :: {non_neg_integer(), [Backend.t()]}
  def delete_user_backends(%User{id: user_id}) do
    ${query}
    |> Repo.delete_all()
  end
end
`;

test("diff highlight: a hunk starting on a closing heredoc", async ({ page }) => {
  const f = fixture();
  const treq = (...args: string[]) =>
    execFileSync(appBinary, args, {
      cwd: f.repo,
      env: {
        ...appEnv({ ...f, bin: join(f.root, "bin") }),
        TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
      },
      stdio: "pipe",
    });
  mkdirSync(dirname(join(f.repo, LOCK_FILE)), { recursive: true });
  writeFileSync(join(f.repo, LOCK_FILE), backends('from(sb in "sources_backends", where: sb.user_id == ^user_id)'));
  git("add", LOCK_FILE);
  git("commit", "-m", "feat: delete a user's backends");
  treq("add", LOCK_BRANCH);
  writeInWorkspace(
    f.repo,
    LOCK_BRANCH,
    LOCK_FILE,
    backends('from(u in User, where: u.id == ^user_id, lock: "FOR UPDATE")'),
  );
  treq("commit", LOCK_BRANCH, "-m", LOCK_COMMIT);

  await openWorkspace(page, LOCK_BRANCH);
  await openTab(page, "Commits");
  await settle(page, 1500);
  const unfolded = page.getByRole("button", { name: "Edit description" });
  if ((await unfolded.count()) === 0) {
    await page.getByRole("button", { name: LOCK_COMMIT, exact: true }).click({ position: { x: 80, y: 12 } });
  }
  await expect(unfolded).toBeVisible();
  await settle(page, 2000);
  await capture(page, {
    name: "20-diff-highlight-02-commits-closing-heredoc",
    title: "Commits tab: backends.ex hunk opening on a closing \"\"\"",
    expectations: [
      "The hunk's first line is the docstring's closing \"\"\", colored as a string",
      "@spec and def after it are colored as code (keywords blue), not all string colored",
      "\"sources_backends\" and \"FOR UPDATE\" are string colored; from, in, where are not",
    ],
  });

  await openTab(page, "Changes");
  await settle(page, 1500);
  await capture(page, {
    name: "20-diff-highlight-03-changes-closing-heredoc",
    title: "Changes tab: the same hunk",
    expectations: [
      "The closing \"\"\" is string colored and the lines after it are colored as code",
      "Only the two quoted literals on the changed lines are string colored",
    ],
  });
});
