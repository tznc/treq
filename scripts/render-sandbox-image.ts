// Renders the setup scripts baked into the base sandbox image
// (sandbox-image/) from the same code the control plane runs at boot, so the
// image and a bootstrapped machine never drift.
//
// Usage: deno run --allow-read --allow-write scripts/render-sandbox-image.ts

import { bootstrapScript, CURRENT_MANIFEST_VERSION } from "../supabase/functions/_shared/remote/boot-manifest.ts";
import { installSshdCommand } from "../supabase/functions/_shared/remote/ssh-vm-config.ts";

/** File name → content, relative to sandbox-image/. */
export function renderSandboxImageFiles(): Record<string, string> {
  return {
    "bootstrap.sh": bootstrapScript(CURRENT_MANIFEST_VERSION),
    "install-sshd.sh": installSshdCommand()[2],
  };
}

if (import.meta.main) {
  const root = new URL("../sandbox-image/", import.meta.url);
  for (const [name, content] of Object.entries(renderSandboxImageFiles())) {
    await Deno.writeTextFile(new URL(name, root), content);
  }
}
