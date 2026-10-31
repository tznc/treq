// Pins a treq CLI release's digests into the machine boot manifests
// (supabase/functions/_shared/remote/boot-manifest.ts). The release workflow
// runs it, then opens a PR with the result.
//
// Usage: deno run --allow-read --allow-write scripts/bump-boot-manifest.ts <version> <sha256-x86_64> <sha256-aarch64>
//
// A manifest that already names the version gets its placeholder digests
// filled; otherwise the next manifest is appended, copied from the current
// one. Real digests are never overwritten.

export interface Digests {
  x86_64: string;
  aarch64: string;
}

const ENTRY = /^ {2}(\d+): \{\n([\s\S]*?)^ {2}\},\n/gm;

export function bumpManifestSource(source: string, version: string, digests: Digests): string {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`bad version: ${version}`);
  for (const d of [digests.x86_64, digests.aarch64]) {
    if (!/^[0-9a-f]{64}$/.test(d)) throw new Error(`bad sha256: ${d}`);
  }
  const pinned = `treq_sha256: { x86_64: "${digests.x86_64}", aarch64: "${digests.aarch64}" },`;
  const entries = [...source.matchAll(ENTRY)].map((m) => ({ key: Number(m[1]), text: m[0], body: m[2] }));
  if (entries.length === 0) throw new Error("no manifests found");

  const matching = entries.filter((e) => e.body.includes(`treq_version: "${version}",`));
  if (matching.length > 0) {
    let out = source;
    for (const e of matching) {
      if (e.body.includes(pinned)) continue;
      if (!e.body.includes('"PLACEHOLDER')) throw new Error(`manifest ${e.key} already pins other digests for ${version}`);
      const filled = e.text
        .replace(/^ *\/\/ TODO\(ops\).*\n/m, "")
        .replace(/treq_sha256: \{[^}]*\},/, pinned);
      out = out.replace(e.text, filled);
    }
    return out;
  }

  const current = Number(/CURRENT_MANIFEST_VERSION = (\d+);/.exec(source)?.[1]);
  const base = entries.find((e) => e.key === current);
  if (!base) throw new Error(`no manifest ${current}`);
  const next = current + 1;
  const body = base.body
    .split("\n")
    .filter((line) => !/^\s*\/\//.test(line))
    .join("\n")
    .replace(/manifest_version: \d+,/, `manifest_version: ${next},`)
    .replace(/treq_version: "[^"]*",/, `treq_version: "${version}",`)
    .replace(/treq_sha256: \{[^}]*\},/, pinned);
  const last = entries[entries.length - 1];
  return source
    .replace(`CURRENT_MANIFEST_VERSION = ${current};`, `CURRENT_MANIFEST_VERSION = ${next};`)
    .replace(last.text, `${last.text}  ${next}: {\n${body}  },\n`);
}

if (import.meta.main) {
  const [version, x86_64, aarch64] = Deno.args;
  const path = new URL("../supabase/functions/_shared/remote/boot-manifest.ts", import.meta.url);
  const source = await Deno.readTextFile(path);
  await Deno.writeTextFile(path, bumpManifestSource(source, version, { x86_64, aarch64 }));
}
