import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";

// Test discovery runs every *.test.ts here; shared fakes live in helpers/.
Deno.test("tests root holds only *.test.ts files; helpers live in helpers/", () => {
  const stray = [...Deno.readDirSync(new URL(".", import.meta.url))]
    .filter((e) => e.isFile && !e.name.endsWith(".test.ts"))
    .map((e) => e.name)
    .sort();
  assertEquals(stray, []);
});
