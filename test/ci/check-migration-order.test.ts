import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SCRIPT = join(import.meta.dirname, "../../scripts/check-migration-order.sh");

function repoWith(base: string[], added: string[]) {
  const dir = mkdtempSync(join(tmpdir(), "mig-order-"));
  const git = (...a: string[]) => execFileSync("git", a, { cwd: dir });
  git("init", "-q", "-b", "main");
  git("config", "user.email", "t@t");
  git("config", "user.name", "t");
  mkdirSync(join(dir, "supabase/migrations"), { recursive: true });
  for (const f of base) writeFileSync(join(dir, "supabase/migrations", f), "");
  git("add", "-A");
  git("commit", "-qm", "base", "--allow-empty");
  git("checkout", "-qb", "pr");
  for (const f of added) writeFileSync(join(dir, "supabase/migrations", f), "");
  git("add", "-A");
  git("commit", "-qm", "pr", "--allow-empty");
  return spawnSync("bash", [SCRIPT, "main"], { cwd: dir, encoding: "utf8" });
}

describe("check-migration-order.sh", () => {
  it("passes when new migrations sort after main's latest", () => {
    expect(repoWith(["042_a.sql", "043_b.sql"], ["044_c.sql"]).status).toBe(0);
  });

  it("passes when no migrations are added", () => {
    expect(repoWith(["042_a.sql"], []).status).toBe(0);
  });

  it("fails when a new migration sorts before main's latest", () => {
    const r = repoWith(["042_a.sql", "043_b.sql"], ["039_old.sql"]);
    expect(r.status).toBe(1);
    expect(r.stdout + r.stderr).toContain("039_old.sql");
  });

  it("fails when a new migration reuses main's latest number", () => {
    expect(repoWith(["043_b.sql"], ["043_dup.sql"]).status).toBe(1);
  });
});
