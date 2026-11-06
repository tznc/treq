// The base sandbox image (sandbox-image/) bakes in the same setup a machine
// gets at boot, so a vendor that needs an image skips it. The committed
// scripts must match what the control plane renders.
//
// Run with: `deno test --allow-read supabase/functions/tests/sandbox_image.test.ts`

import { assertEquals, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { renderSandboxImageFiles } from "../../../scripts/render-sandbox-image.ts";

const root = new URL("../../../sandbox-image/", import.meta.url);

Deno.test("committed image scripts match the rendered boot and sshd setup", async () => {
  for (const [name, content] of Object.entries(renderSandboxImageFiles())) {
    assertEquals(
      await Deno.readTextFile(new URL(name, root)),
      content,
      `${name} is stale: run deno run --allow-read --allow-write scripts/render-sandbox-image.ts`,
    );
  }
});

Deno.test("the Dockerfile runs both setup scripts on a configurable base and exposes the gateway port", async () => {
  const dockerfile = await Deno.readTextFile(new URL("Dockerfile", root));
  assertStringIncludes(dockerfile, "bootstrap.sh");
  assertStringIncludes(dockerfile, "install-sshd.sh");
  assertStringIncludes(dockerfile, "ARG BASE_IMAGE=");
  assertStringIncludes(dockerfile, "EXPOSE 8022");
});
