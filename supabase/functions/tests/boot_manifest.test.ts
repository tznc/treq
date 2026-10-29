// The machine bootstrap must really install the treq CLI: `treq gateway`
// (the in-machine gateway) and the remote PTY supervisor both run it.
//
// Run with: `deno test supabase/functions/tests/boot_manifest.test.ts`

import { assert, assertEquals, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { bootstrapScript, CURRENT_MANIFEST_VERSION, manifestForVersion } from "../_shared/remote/boot-manifest.ts";
import { TREQ_BINARY } from "../_shared/remote/ssh-vm-config.ts";

const script = bootstrapScript(CURRENT_MANIFEST_VERSION);
const version = manifestForVersion(CURRENT_MANIFEST_VERSION)!.treq_version;

Deno.test("bootstrap downloads the treq CLI release for the machine's arch", () => {
  assertStringIncludes(script, `TREQ_VERSION="${version}"`);
  assertStringIncludes(
    script,
    "https://github.com/tznc/treq/releases/download/v$TREQ_VERSION/treq-cli-$TREQ_VERSION-linux-$ARCH.tar.gz",
  );
  assertStringIncludes(script, "x86_64|amd64) ARCH=x86_64");
  assertStringIncludes(script, "aarch64|arm64) ARCH=aarch64");
});

Deno.test("bootstrap checks the archive against the pinned digest before installing", () => {
  const pinned = manifestForVersion(CURRENT_MANIFEST_VERSION)!.treq_sha256;
  assertStringIncludes(script, `TREQ_SHA256_X86_64="${pinned.x86_64}"`);
  assertStringIncludes(script, `TREQ_SHA256_AARCH64="${pinned.aarch64}"`);
  // The digest comes from the control plane, not from the same release.
  assert(!script.includes(".tar.gz.sha256"), "no downloaded checksum");
  assertStringIncludes(script, `echo "$expected  $tmp/treq.tar.gz" | sha256sum -c -`);
  const check = script.indexOf("sha256sum -c");
  const download = script.indexOf(`-o "$tmp/treq.tar.gz"`);
  const extract = script.indexOf(`tar -xzf`);
  assert(download > 0 && check > download && extract > check, "the checksum is verified before extraction");
});

Deno.test("bootstrap fails closed while a pinned digest is a placeholder", () => {
  assertStringIncludes(script, `PLACEHOLDER*) echo "treq bootstrap: no pinned sha256 for treq $TREQ_VERSION $ARCH" >&2; exit 1 ;;`);
  const guard = script.indexOf("PLACEHOLDER*)");
  assert(guard > 0 && guard < script.indexOf("curl -fsSL"), "refuses before downloading");
});

Deno.test("every manifest pins a digest per arch", () => {
  for (let v = 1; v <= CURRENT_MANIFEST_VERSION; v++) {
    const m = manifestForVersion(v)!;
    for (const d of [m.treq_sha256.x86_64, m.treq_sha256.aarch64]) {
      assert(/^[0-9a-f]{64}$/.test(d) || d.startsWith("PLACEHOLDER"), `bad digest ${d}`);
    }
  }
});

Deno.test("bootstrap installs treq where the gateway service runs it", () => {
  assertStringIncludes(script, `-C ${TREQ_BINARY.replace(/\/treq$/, "")} treq`);
});

Deno.test("bootstrap skips the download when that treq version is installed", () => {
  assertStringIncludes(script, `"$(${TREQ_BINARY} --version 2>/dev/null)" = "treq $TREQ_VERSION"`);
});

// Boat and Daytona exec as a non-root user, so every write outside $HOME
// goes through $SUDO, which must be defined before the first one.
Deno.test("bootstrap defines SUDO before it writes outside the home directory", () => {
  const sudo = script.indexOf('SUDO=""');
  assert(sudo > 0, "SUDO is defined");
  assertStringIncludes(script, "$SUDO mkdir -p /var/lib/treq");
  assert(!/^mkdir -p \/var\/lib\/treq/m.test(script), "no bare mkdir of /var/lib/treq");
  assert(sudo < script.indexOf("mkdir -p /var/lib/treq"), "SUDO comes first");
  assertStringIncludes(script, '| $SUDO tee "$STATE_FILE"');
  assert(!script.includes('> "$STATE_FILE"'), "the state file is never written without $SUDO");
});

Deno.test("bootstrap really installs jj from the static musl release", () => {
  assertStringIncludes(
    script,
    "https://github.com/jj-vcs/jj/releases/download/v$JJ_VERSION/jj-v$JJ_VERSION-$ARCH-unknown-linux-musl.tar.gz",
  );
  // `jj --version` prints `jj 0.40.0-<commit>`: match the version prefix.
  assertStringIncludes(script, `case "$(/usr/local/bin/jj --version 2>/dev/null)" in "jj $JJ_VERSION"|"jj $JJ_VERSION-"*)`);
  assertStringIncludes(script, "-C /usr/local/bin ./jj");
});

Deno.test("bootstrap installs git with apt when it is missing", () => {
  assertStringIncludes(script, "command -v git");
  assertStringIncludes(script, "$SUDO apt-get install -y git");
});

Deno.test("bootstrap records the manifest only after the real installs, not echo-only steps", () => {
  assert(!script.includes("install_binary"), "no echo-only binary installs");
  const state = script.indexOf('| $SUDO tee "$STATE_FILE"');
  for (const step of ["\ninstall_treq\n", "\ninstall_jj\n", "\ninstall_git\n"]) {
    const at = script.indexOf(step);
    assert(at > 0 && at < state, `${step.trim()} runs before the state is written`);
  }
});

Deno.test("the manifest version moves past the one that recorded phantom installs", () => {
  assert(CURRENT_MANIFEST_VERSION >= 4);
});

Deno.test("bootstrap checks jj against its pinned digest before extracting", () => {
  const m = manifestForVersion(CURRENT_MANIFEST_VERSION)!;
  assertEquals(m.jj_sha256, {
    x86_64: "5c8979f46873e052f59bdd9535636dca6e6f9f70571b73f6d63c3b92acfaa037",
    aarch64: "b26f24ff7a34838fbafe8788e6a94a9cdcf51601ef8c9af8fab4fa22c06ddbee",
  });
  assertStringIncludes(script, `JJ_SHA256_X86_64="${m.jj_sha256.x86_64}"`);
  const check = script.indexOf(`sha256sum -c`, script.indexOf("install_jj() {"));
  const extract = script.indexOf(`"$tmp/jj.tar.gz" -C /usr/local/bin`);
  assert(check > 0 && check < extract, "jj's checksum is verified before extraction");
});
