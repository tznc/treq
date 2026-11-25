// Unit tests for the exec-based pidfile supervisor (provider-http.ts),
// running its script in a local shell.
//
// Run with: `deno test supabase/functions/tests/pidfile_service.test.ts`

import { assertMatch, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { ensurePidfileService } from "../_shared/remote/provider-http.ts";
import { ProviderError } from "../_shared/remote/provider.ts";

const text = new TextDecoder();

// Runs the supervisor's `sh -c <script>` here, in bash.
async function localExec(command: string[]) {
  const out = await new Deno.Command("bash", { args: command.slice(1) }).output();
  return { exitCode: out.code, stdout: text.decode(out.stdout), stderr: text.decode(out.stderr) };
}

Deno.test("a service that exits at start reports the end of its log", async () => {
  const dir = await Deno.makeTempDir();
  const err = await assertRejects(
    () => ensurePidfileService(localExec, "treq-sshd", { cmd: `${dir}/missing-launcher`, args: [] }, { vendor: "Boat", dir }),
    ProviderError,
  );
  assertMatch(err.message, /^Boat service treq-sshd failed to start: .*missing-launcher.*No such file or directory/s);
});
