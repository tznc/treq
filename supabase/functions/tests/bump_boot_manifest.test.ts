// The release workflow pins each treq CLI release's digests into
// boot-manifest.ts through bumpManifestSource. No network.
//
// Run with: `deno test --allow-read supabase/functions/tests/bump_boot_manifest.test.ts`

import { assertEquals, assertStringIncludes, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { bumpManifestSource } from "../../../scripts/bump-boot-manifest.ts";

const X86 = "a".repeat(64);
const ARM = "b".repeat(64);

const SOURCE = `export const CURRENT_MANIFEST_VERSION = 2;

const MANIFESTS: Record<number, BootManifest> = {
  1: {
    manifest_version: 1,
    treq_version: "0.3.0",
    treq_sha256: { x86_64: "${"c".repeat(64)}", aarch64: "${"d".repeat(64)}" },
    jj_version: "0.40.0",
  },
  // The first manifest with treq 0.4.0.
  2: {
    manifest_version: 2,
    treq_version: "0.4.0",
    // TODO(ops): pin the digests.
    treq_sha256: { x86_64: "PLACEHOLDER-treq-0.4.0-x86_64", aarch64: "PLACEHOLDER-treq-0.4.0-aarch64" },
    jj_version: "0.40.0",
    agents: [{ name: "claude", version: "1.0.0" }],
  },
};
`;

Deno.test("a release whose version a manifest already names fills its placeholder digests", () => {
  const out = bumpManifestSource(SOURCE, "0.4.0", { x86_64: X86, aarch64: ARM });
  assertStringIncludes(out, `treq_sha256: { x86_64: "${X86}", aarch64: "${ARM}" },`);
  assertEquals(out.includes("PLACEHOLDER"), false);
  assertEquals(out.includes("TODO(ops)"), false);
  assertStringIncludes(out, "export const CURRENT_MANIFEST_VERSION = 2;");
});

Deno.test("a new release appends the next manifest from the current one", () => {
  const filled = bumpManifestSource(SOURCE, "0.4.0", { x86_64: X86, aarch64: ARM });
  const out = bumpManifestSource(filled, "0.5.0", { x86_64: "e".repeat(64), aarch64: "f".repeat(64) });
  assertStringIncludes(out, "export const CURRENT_MANIFEST_VERSION = 3;");
  assertStringIncludes(
    out,
    `  3: {
    manifest_version: 3,
    treq_version: "0.5.0",
    treq_sha256: { x86_64: "${"e".repeat(64)}", aarch64: "${"f".repeat(64)}" },
    jj_version: "0.40.0",
    agents: [{ name: "claude", version: "1.0.0" }],
  },
};`,
  );
  // Earlier manifests are left alone.
  assertStringIncludes(out, `treq_sha256: { x86_64: "${X86}", aarch64: "${ARM}" },`);
});

Deno.test("re-running for a release already pinned changes nothing", () => {
  const filled = bumpManifestSource(SOURCE, "0.4.0", { x86_64: X86, aarch64: ARM });
  assertEquals(bumpManifestSource(filled, "0.4.0", { x86_64: X86, aarch64: ARM }), filled);
});

Deno.test("a pinned release with different digests is refused, never overwritten", () => {
  const filled = bumpManifestSource(SOURCE, "0.4.0", { x86_64: X86, aarch64: ARM });
  assertThrows(() => bumpManifestSource(filled, "0.4.0", { x86_64: "9".repeat(64), aarch64: ARM }), Error, "already pins");
});

Deno.test("malformed versions and digests are refused", () => {
  assertThrows(() => bumpManifestSource(SOURCE, "v0.5", { x86_64: X86, aarch64: ARM }), Error, "version");
  assertThrows(() => bumpManifestSource(SOURCE, "0.5.0", { x86_64: "nope", aarch64: ARM }), Error, "sha256");
});

Deno.test("the real boot-manifest.ts parses", async () => {
  const real = await Deno.readTextFile(new URL("../_shared/remote/boot-manifest.ts", import.meta.url));
  const out = bumpManifestSource(real, "9.9.9", { x86_64: X86, aarch64: ARM });
  assertStringIncludes(out, `treq_version: "9.9.9"`);
});
