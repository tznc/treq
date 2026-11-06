// Renders the setup scripts baked into the base sandbox image
// (sandbox-image/) from the same code the control plane runs at boot, so the
// image and a bootstrapped machine never drift.
//
// Usage:
//   deno run --allow-read --allow-write scripts/render-sandbox-image.ts
//   deno run --allow-read --allow-write scripts/render-sandbox-image.ts --treq-version <v>
//
// --treq-version is for image builds that bake in a treq binary (bin/treq):
// bootstrap then finds that version installed and skips the download. The
// committed files are always rendered without it.

import {
  bootstrapScript,
  type BootstrapOverrides,
  CURRENT_MANIFEST_VERSION,
} from "../supabase/functions/_shared/remote/boot-manifest.ts";
import { installSshdCommand } from "../supabase/functions/_shared/remote/ssh-vm-config.ts";

/** File name → content, relative to sandbox-image/. */
export function renderSandboxImageFiles(overrides: BootstrapOverrides = {}): Record<string, string> {
  return {
    "bootstrap.sh": bootstrapScript(CURRENT_MANIFEST_VERSION, overrides),
    "install-sshd.sh": installSshdCommand()[2],
  };
}

if (import.meta.main) {
  const at = Deno.args.indexOf("--treq-version");
  const treqVersion = at >= 0 ? Deno.args[at + 1] : undefined;
  if (at >= 0 && !/^[0-9A-Za-z.+-]+$/.test(treqVersion ?? "")) throw new Error("--treq-version needs a version");
  const root = new URL("../sandbox-image/", import.meta.url);
  for (const [name, content] of Object.entries(renderSandboxImageFiles({ treqVersion }))) {
    await Deno.writeTextFile(new URL(name, root), content);
  }
}
