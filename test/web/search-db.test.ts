import { createRequire } from "node:module";
import { execSearch } from "../../web/src/utils/searchDb";

const require = createRequire(new URL("../../web/package.json", import.meta.url));
const initSqlJs = require("sql.js");

async function dbWith(rows: [string, string, string][]) {
  const SQL = await initSqlJs();
  const db = new SQL.Database();
  db.run("CREATE VIRTUAL TABLE docs_fts USING fts4(title, body, url)");
  for (const r of rows) db.run("INSERT INTO docs_fts(title, body, url) VALUES (?, ?, ?)", r);
  return db;
}

describe("execSearch", () => {
  it("ranks the best match into the results even when weaker matches were indexed first", async () => {
    const weak: [string, string, string][] = Array.from({ length: 10 }, (_, i) => [
      `Page ${i}`,
      "mentions the cli once, a workspace once, and its status once",
      `/docs/weak-${i}`,
    ]);
    const db = await dbWith([
      ...weak,
      ["CLI", "treq cli: workspace status. Show workspace status. cli status workspace", "/docs/reference/cli"],
    ]);

    const urls = execSearch(db, "cli workspace status").map((r) => r.url);

    expect(urls).toHaveLength(8);
    expect(urls[0]).toBe("/docs/reference/cli");
  });
});
