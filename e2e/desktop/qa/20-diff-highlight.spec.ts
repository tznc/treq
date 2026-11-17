// Diff syntax highlighting across hunk lines: an Elixir `@doc` heredoc
// colors every line it spans, the closing `"""` included, and code after
// it is code again. The file row is not observable, so the step clicks a
// fixed point (the only uncommitted file, as in 14-diff-context).
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { fixture } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";
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
      "The @doc heredoc's body line is string colored, not Compares as a module",
      "The closing \"\"\" has the same color as the body and opening \"\"\"",
      "@spec and def after the heredoc are colored as code again",
    ],
  });
});
