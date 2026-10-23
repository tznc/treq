import { describe, expect, it, vi } from "vitest";
import {
  consumeDesktopToken,
  exchangeDesktopToken,
  type ExchangeDeps,
  type TokenTable,
} from "../../supabase/functions/exchange-desktop-token/lib.ts";

const NOW = new Date("2026-10-23T12:00:00Z");
const SESSION = {
  access_token: "at",
  refresh_token: "rt",
  expires_in: 3600,
  user: { id: "u1" },
};

function deps(overrides: Partial<ExchangeDeps> = {}): ExchangeDeps {
  return {
    consumeToken: vi.fn(async () => "u1"),
    createSession: vi.fn(async () => SESSION),
    now: () => NOW,
    ...overrides,
  };
}

/** In-memory table that applies the filters the way one UPDATE … RETURNING would. */
function fakeTable(
  rows: { token: string; used: boolean; expires_at: string; user_id: string }[],
) {
  const filters: [string, string, unknown][] = [];
  let patch: Record<string, unknown> = {};
  const builder = {
    update(p: Record<string, unknown>) {
      patch = p;
      return builder;
    },
    eq(col: string, val: unknown) {
      filters.push(["eq", col, val]);
      return builder;
    },
    gt(col: string, val: unknown) {
      filters.push(["gt", col, val]);
      return builder;
    },
    select() {
      return builder;
    },
    async maybeSingle() {
      const row = rows.find((r) =>
        filters.every(([op, col, val]) => {
          const v = (r as Record<string, unknown>)[col];
          return op === "eq" ? v === val : String(v) > String(val);
        }),
      );
      if (row) Object.assign(row, patch);
      filters.length = 0;
      return { data: row ? { user_id: row.user_id } : null, error: null };
    },
  };
  return { table: builder as unknown as TokenTable, rows };
}

describe("consumeDesktopToken", () => {
  const future = "2026-10-23T12:05:00.000Z";

  it("returns the user once and refuses a second use", async () => {
    const { table } = fakeTable([
      { token: "t1", used: false, expires_at: future, user_id: "u1" },
    ]);
    expect(await consumeDesktopToken(table, "t1", NOW)).toBe("u1");
    expect(await consumeDesktopToken(table, "t1", NOW)).toBeNull();
  });

  it("refuses an expired token without marking it used", async () => {
    const { table, rows } = fakeTable([
      {
        token: "t1",
        used: false,
        expires_at: "2026-10-23T11:59:00.000Z",
        user_id: "u1",
      },
    ]);
    expect(await consumeDesktopToken(table, "t1", NOW)).toBeNull();
    expect(rows[0].used).toBe(false);
  });
});

describe("exchangeDesktopToken", () => {
  it("rejects a missing token", async () => {
    const res = await exchangeDesktopToken(deps(), {});
    expect(res.status).toBe(400);
  });

  it("returns 401 when the token cannot be consumed", async () => {
    const d = deps({ consumeToken: vi.fn(async () => null) });
    const res = await exchangeDesktopToken(d, { token: "t1" });
    expect(res.status).toBe(401);
    expect(d.createSession).not.toHaveBeenCalled();
  });

  it("returns the session for a valid token", async () => {
    const d = deps();
    const res = await exchangeDesktopToken(d, { token: "t1" });
    expect(res).toEqual({ status: 200, body: SESSION });
    expect(d.consumeToken).toHaveBeenCalledWith("t1", NOW);
    expect(d.createSession).toHaveBeenCalledWith("u1");
  });

  it("hides internal error messages", async () => {
    const d = deps({
      createSession: vi.fn(async () => {
        throw new Error("relation desktop_auth_tokens does not exist");
      }),
    });
    const res = await exchangeDesktopToken(d, { token: "t1" });
    expect(res.status).toBe(500);
    expect(JSON.stringify(res.body)).not.toContain("desktop_auth_tokens");
  });
});
