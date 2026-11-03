import { describe, expect, it } from "vitest";
import { isResumable } from "../../supabase/functions/_shared/remote/instance-actions.ts";

const NOW = Date.parse("2026-11-03T12:00:00Z");
const row = (minutesAgo: number) =>
  ({ status: "provisioning", updated_at: new Date(NOW - minutesAgo * 60_000).toISOString() }) as never;

describe("a provisioning row is stuck after 5 minutes", () => {
  it("resumes a row untouched for 5 minutes", () => {
    expect(isResumable(row(5), NOW)).toBe(true);
  });
  it("leaves a row updated 4 minutes ago alone", () => {
    expect(isResumable(row(4), NOW)).toBe(false);
  });
});
