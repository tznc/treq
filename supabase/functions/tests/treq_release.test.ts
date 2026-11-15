// Which treq CLI release a machine bootstraps: the newest published GitHub
// release with both Linux CLI assets, reviewed manifest pins first.
// No network: fetch is a fake.

import { assert, assertEquals, assertRejects, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { type BootManifest, bootstrapScript, CURRENT_MANIFEST_VERSION, manifestForVersion } from "../_shared/remote/boot-manifest.ts";
import { createTreqReleaseResolver } from "../_shared/remote/treq-release.ts";
import { runBootManifest } from "../_shared/remote/machine-setup.ts";
import { ProviderError, type SandboxProvider } from "../_shared/remote/provider.ts";
import { defined } from "./helpers/remote_fakes.ts";

const X86 = "5d01af2aa81372a3e61cfb4b291393e6fb89f990554b162ba7cced241e1a04f3";
const ARM = "3931e7f99caea1684d40d087369137040be5ab517a73830025195dfd7f11c87a";

function cliAssets(v: string, arches = ["x86_64", "aarch64"]) {
  return arches.map((arch) => ({
    name: `treq-cli-${v}-linux-${arch}.tar.gz`,
    digest: `sha256:${arch === "x86_64" ? X86 : ARM}`,
  }));
}

function release(tag: string, opts: { draft?: boolean; prerelease?: boolean; assets?: unknown[] } = {}) {
  return {
    tag_name: tag,
    draft: opts.draft ?? false,
    prerelease: opts.prerelease ?? false,
    assets: opts.assets ?? cliAssets(tag.replace(/^v/, "")),
  };
}

function fakeFetch(answer: unknown[] | Response | Error) {
  const urls: string[] = [];
  const fetch = ((input: string | URL | Request) => {
    urls.push(String(input));
    if (answer instanceof Error) return Promise.reject(answer);
    if (answer instanceof Response) return Promise.resolve(answer.clone());
    return Promise.resolve(Response.json(answer));
  }) as typeof globalThis.fetch;
  return { fetch, urls };
}

/** The current manifest with placeholder treq digests, as on main today. */
const unpinned: BootManifest = { ...defined(manifestForVersion(CURRENT_MANIFEST_VERSION)), treq_version: "0.4.0" };
const resolverFor = (answer: Parameters<typeof fakeFetch>[0], manifests = [unpinned], now = () => 0) =>
  createTreqReleaseResolver({ fetch: fakeFetch(answer).fetch, manifests, now });

Deno.test("the newest published release wins, by semver", async () => {
  const resolve = resolverFor([release("v0.2.0"), release("v0.10.0"), release("v0.9.0")]);
  const r = await resolve();
  assertEquals(r.version, "0.10.0");
  assertEquals(r.sha256, { x86_64: X86, aarch64: ARM });
  assertEquals(r.source, "github");
});

Deno.test("a draft is skipped", async () => {
  const r = await resolverFor([release("v0.4.0", { draft: true }), release("v0.3.0")])();
  assertEquals(r.version, "0.3.0");
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
  const noDigest = cliAssets("0.4.1").map((a) => ({ ...a, digest: null }));
  const r = await resolverFor([
    release("v0.4.1", { assets: noDigest }),
    release("v0.4.0", { assets: cliAssets("0.4.0", ["x86_64"]) }),
    release("v0.3.0"),
  ])();
  assertEquals(r.version, "0.3.0");
});

Deno.test("reviewed manifest pins beat the digests GitHub reports", async () => {
  const pinned = { ...unpinned, treq_version: "0.3.0", treq_sha256: { x86_64: "a".repeat(64), aarch64: "b".repeat(64) } };
  const r = await resolverFor([release("v0.3.0")], [pinned, unpinned])();
  assertEquals(r, { version: "0.3.0", sha256: pinned.treq_sha256, source: "manifest" });
});

Deno.test("a GitHub API failure throws a ProviderError when the manifest has no real pins", async () => {
  for (const answer of [new Error("network down"), new Response("rate limited", { status: 403 }), []]) {
    const err = await assertRejects(() => resolverFor(answer)(), ProviderError);
    assertEquals(err.kind, "unavailable");
    assertStringIncludes(err.message, "treq");
  }
});

Deno.test("a GitHub API failure falls back to the current manifest when it has real pins", async () => {
  const pinned = { ...unpinned, treq_sha256: { x86_64: "a".repeat(64), aarch64: "b".repeat(64) } };
  const r = await resolverFor(new Response("boom", { status: 500 }), [pinned])();
  assertEquals(r, { version: "0.4.0", sha256: pinned.treq_sha256, source: "manifest" });
});

Deno.test("the resolved release is cached for about ten minutes", async () => {
  const { fetch, urls } = fakeFetch([release("v0.3.0")]);
  let t = 0;
  const resolve = createTreqReleaseResolver({ fetch, manifests: [unpinned], now: () => t });
  await resolve();
  t = 9 * 60_000;
  await resolve();
  assertEquals(urls.length, 1);
  t = 11 * 60_000;
  await resolve();
  assertEquals(urls.length, 2);
  assertStringIncludes(urls[0], "https://api.github.com/repos/tznc/treq/releases");
});

Deno.test("the bootstrap script carries the resolved version and digests, no placeholder", async () => {
  const execs: string[][] = [];
  const provider = {
    execOnMachine: (_id: string, cmd: string[]) => {
      execs.push(cmd);
      return Promise.resolve({ exitCode: 0, stdout: "", stderr: "" });
    },
  } as unknown as SandboxProvider;
  await runBootManifest(provider, "m1", "bootstrap", resolverFor([release("v0.4.0", { draft: true }), release("v0.3.0")]));
  const script = defined(execs[0])[2];
  assertStringIncludes(script, 'TREQ_VERSION="0.3.0"');
  assertStringIncludes(script, `TREQ_SHA256_X86_64="${X86}"`);
  assertStringIncludes(script, `TREQ_SHA256_AARCH64="${ARM}"`);
  assert(!/="PLACEHOLDER/.test(script), "no placeholder digest");
  // Without overrides the script still renders the manifest's own values.
  assertStringIncludes(bootstrapScript(CURRENT_MANIFEST_VERSION), `TREQ_VERSION="${unpinned.treq_version}"`);
});
